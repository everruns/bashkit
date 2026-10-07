//! Redirection handling (`>`, `>>`, `<`, `<<<`, fd duplication, fd table).
//!
//! Split out of interpreter/mod.rs: input-redirection collection and the
//! output-redirection / fd-routing core. The `FdTarget` enum and
//! `route_fd_table_content` helper stay in the parent module (referenced
//! by interpreter state fields).
//!
//! Redirection failures are reported as `bash: <path>: <reason>` via
//! [`crate::error::io_error_reason`] — never as the `Display` of a
//! [`crate::Error`],
//! whose `io error: ` prefix is a Rust enum shape no shell ever prints.
//!
//! The reason text matters beyond cosmetics: the fuzz/proptest leak detector
//! in `bashkit::testing` suppresses shell echoes of user input only for lines
//! matching a recognized real-shell template. Nightly `glob_fuzz` run 218
//! failed on `</r\0ustc/` — the NUL is dropped during expansion, so the
//! redirect target became `/rustc/` after the target's own input pre-filter
//! had run, and `bash: /rustc/: io error: file not found` was reported as a
//! TM-INF-016 host-path leak instead of an echo.

use super::*;
use crate::error::io_error_reason;

impl Interpreter {
    /// A target written with a trailing slash names a directory, so an
    /// output redirect to it fails without creating anything. `resolve_path`
    /// drops the slash, so the check has to run on the word as written.
    fn rejects_directory_target(&self, target: &str, result: &mut ExecResult) -> bool {
        if !target.ends_with('/') {
            return false;
        }
        result.stdout = crate::StreamData::new();
        result.stderr = self.redirect_error(target, "Is a directory").into();
        result.exit_code = 1;
        true
    }

    /// `bash: line N: PATH: reason` — the shape bash uses for a redirection
    /// that could not be opened. Bash names the script instead of `bash`
    /// when running a file; the interpreter does not track a script path.
    fn redirect_error(&self, path: &str, reason: &str) -> String {
        format!("bash: line {}: {path}: {reason}\n", self.current_line)
    }

    /// Process input redirections (< file, <<< string)
    pub(super) async fn process_input_redirections(
        &mut self,
        existing_stdin: Option<crate::StreamData>,
        redirects: &[Redirect],
    ) -> Result<Option<crate::StreamData>> {
        let mut stdin = existing_stdin;

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Input => {
                    if !self.shell_features.has_file_redirects()
                        && !word_is_literal_dev_null(&redirect.target)
                    {
                        return Err(crate::error::Error::Execution(format!(
                            "bash: {}: filesystem redirection disabled",
                            redirect_target_label(&redirect.target)
                        )));
                    }
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Handle /dev/null at interpreter level - cannot be bypassed
                    if dev_fd_alias(&path) == Some(0) {
                        // `< /dev/stdin` re-reads the current stdin: no-op.
                    } else if is_dev_null(&path) {
                        stdin = Some(crate::StreamData::new()); // EOF
                    } else if !self.shell_features.has_file_redirects() {
                        return Err(crate::error::Error::Execution(format!(
                            "bash: {}: filesystem redirection disabled",
                            target_path
                        )));
                    } else {
                        match self.fs.read_file(&path).await {
                            Ok(content) => {
                                stdin = Some(content.into());
                            }
                            Err(e) => {
                                return Err(crate::error::Error::CommandFailure(
                                    self.redirect_error(&target_path, &io_error_reason(&e)),
                                ));
                            }
                        }
                    }
                }
                RedirectKind::HereString => {
                    // <<< string - use the target as stdin content
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(format!("{}\n", content).into());
                }
                RedirectKind::HereDoc | RedirectKind::HereDocStrip => {
                    // << EOF / <<- EOF - use the heredoc content as stdin.
                    // `3<<EOF` feeds fd 3, which nothing here reads.
                    if matches!(redirect.fd, None | Some(0)) {
                        let content = self.expand_word(&redirect.target).await?;
                        stdin = Some(content.into());
                    }
                }
                RedirectKind::DupInput => {
                    // <&FD - if FD is a coproc read FD, consume next line
                    let target = self.expand_word(&redirect.target).await?;
                    if let Ok(fd) = target.parse::<i32>()
                        && let Some(buf) = self.coproc_buffers.get_mut(&fd)
                    {
                        if let Some(line) = buf.pop() {
                            stdin = Some(format!("{}\n", line).into());
                        } else {
                            stdin = Some(crate::StreamData::new()); // EOF
                        }
                    }
                }
                _ => {
                    // Output redirections handled separately
                }
            }
        }

        Ok(stdin)
    }

    /// Apply output redirections to command output
    pub(super) async fn apply_redirections(
        &mut self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if let Some(stderr) = self.disabled_redirect_error(redirects) {
            result.stdout = crate::StreamData::new();
            result.stderr = stderr.into();
            result.exit_code = 1;
            return Ok(result);
        }

        // Skip the fd-table path when there are no DupOutput redirects mixed
        // with file redirects — the simple single-pass logic is sufficient and
        // avoids any behavioural delta for the common case.
        let has_dup_output = redirects.iter().any(|r| r.kind == RedirectKind::DupOutput);
        let has_file_redirect = redirects.iter().any(|r| {
            matches!(
                r.kind,
                RedirectKind::Output
                    | RedirectKind::Clobber
                    | RedirectKind::Append
                    | RedirectKind::OutputBoth
            )
        });

        // `N>file` (N>=3) opens fd N without touching stdout; only the
        // fd-table path keeps fd N separate from fd 1.
        if (has_dup_output && has_file_redirect) || has_high_fd_file_redirect(redirects) {
            return self.apply_redirections_fd_table(result, redirects).await;
        }

        // --- Fast path: no mixed dup+file redirects ---
        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output | RedirectKind::Clobber => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    if self.rejects_directory_target(&target_path, &mut result) {
                        return Ok(result);
                    }
                    let path = self.resolve_path(&target_path);
                    if let Some(target_fd) = dev_fd_alias(&path) {
                        self.dup_output_fast(&mut result, redirect.fd.unwrap_or(1), target_fd)
                            .await?;
                    } else if is_dev_null(&path) {
                        match redirect.fd {
                            Some(2) => result.stderr = crate::StreamData::new(),
                            _ => result.stdout = crate::StreamData::new(),
                        }
                    } else {
                        if redirect.kind == RedirectKind::Output
                            && self.scoped.variables.get("SHOPT_C").map(|v| v.as_str()) == Some("1")
                            && self.fs.stat(&path).await.is_ok()
                        {
                            result.stdout = crate::StreamData::new();
                            result.stderr = self
                                .redirect_error(&target_path, "cannot overwrite existing file")
                                .into();
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        match redirect.fd {
                            Some(2) => {
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = self
                                        .redirect_error(&target_path, &io_error_reason(&e))
                                        .into();
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = crate::StreamData::new();
                            }
                            _ => {
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stdout.as_bytes()).await
                                {
                                    result.stdout = crate::StreamData::new();
                                    result.stderr = self
                                        .redirect_error(&target_path, &io_error_reason(&e))
                                        .into();
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = crate::StreamData::new();
                            }
                        }
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    if self.rejects_directory_target(&target_path, &mut result) {
                        return Ok(result);
                    }
                    let path = self.resolve_path(&target_path);
                    if let Some(target_fd) = dev_fd_alias(&path) {
                        self.dup_output_fast(&mut result, redirect.fd.unwrap_or(1), target_fd)
                            .await?;
                    } else if is_dev_null(&path) {
                        match redirect.fd {
                            Some(2) => result.stderr = crate::StreamData::new(),
                            _ => result.stdout = crate::StreamData::new(),
                        }
                    } else {
                        match redirect.fd {
                            Some(2) => {
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = self
                                        .redirect_error(&target_path, &io_error_reason(&e))
                                        .into();
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = crate::StreamData::new();
                            }
                            _ => {
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stdout.as_bytes()).await
                                {
                                    result.stdout = crate::StreamData::new();
                                    result.stderr = self
                                        .redirect_error(&target_path, &io_error_reason(&e))
                                        .into();
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = crate::StreamData::new();
                            }
                        }
                    }
                }
                RedirectKind::OutputBoth => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if let Some(target_fd) = dev_fd_alias(&path) {
                        // `&> /dev/fd/N` == `>&N 2>&N`, resolved against the
                        // original descriptors.
                        if target_fd == 2 {
                            let mut combined = std::mem::take(&mut result.stdout);
                            combined.append(&result.stderr);
                            result.stderr = combined;
                        } else {
                            self.dup_output_fast(&mut result, 1, target_fd).await?;
                            self.dup_output_fast(&mut result, 2, target_fd).await?;
                        }
                    } else if is_dev_null(&path) {
                        result.stdout = crate::StreamData::new();
                        result.stderr = crate::StreamData::new();
                    } else {
                        let mut combined = result.stdout.as_bytes().to_vec();
                        combined.extend_from_slice(result.stderr.as_bytes());
                        if let Err(e) = self.fs.write_file(&path, &combined).await {
                            result.stderr = self
                                .redirect_error(&target_path, &io_error_reason(&e))
                                .into();
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        result.stdout = crate::StreamData::new();
                        result.stderr = crate::StreamData::new();
                    }
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let target_fd: i32 = target.parse().unwrap_or(1);
                    let src_fd = redirect.fd.unwrap_or(1);
                    self.dup_output_fast(&mut result, src_fd, target_fd).await?;
                }
                RedirectKind::Input
                | RedirectKind::HereString
                | RedirectKind::HereDoc
                | RedirectKind::HereDocStrip => {}
                RedirectKind::DupInput => {}
            }
        }

        Ok(result)
    }

    /// `src_fd>&target_fd` on the fast (single-pass) redirect path.
    async fn dup_output_fast(
        &mut self,
        result: &mut ExecResult,
        src_fd: i32,
        target_fd: i32,
    ) -> Result<()> {
        // Check exec_fd_table for persistent fd targets. Fd 1 and 2 are the
        // current streams here: `exec >log` is applied where the shell's
        // own output leaves (top level), so `$(cmd 2>&1)` still captures.
        if target_fd >= 3
            && let Some(fd_target) = self.exec_fd_table.get(&target_fd).cloned()
        {
            let data = if src_fd == 2 {
                std::mem::take(&mut result.stderr)
            } else {
                std::mem::take(&mut result.stdout)
            };
            // A saved copy of the original stream (`exec 3>&1 >log`) skips
            // the `exec` routing of fd 1/2.
            let passthrough = match &fd_target {
                FdTarget::Stdout => self.exec_fd_table.contains_key(&1),
                FdTarget::Stderr => self.exec_fd_table.contains_key(&2),
                _ => false,
            };
            match &fd_target {
                FdTarget::Stdout if passthrough => self.exec_passthrough.0.append(&data),
                FdTarget::Stderr if passthrough => self.exec_passthrough.1.append(&data),
                FdTarget::Stdout => result.stdout.append(&data),
                FdTarget::Stderr => result.stderr.append(&data),
                // A closed descriptor accepts nothing.
                FdTarget::DevNull | FdTarget::Closed => {}
                FdTarget::WriteFile(path, _) | FdTarget::AppendFile(path, _) => {
                    self.fs.append_file(path, data.as_bytes()).await?;
                }
            }
        } else {
            match (src_fd, target_fd) {
                (2, 1) => {
                    result.stdout.append(&result.stderr);
                    result.stderr = crate::StreamData::new();
                }
                (1, 2) => {
                    result.stderr.append(&result.stdout);
                    result.stdout = crate::StreamData::new();
                }
                (src, dst) if dst >= 3 => {
                    let data = if src == 2 {
                        std::mem::take(&mut result.stderr)
                    } else {
                        std::mem::take(&mut result.stdout)
                    };
                    if self.pending_fd_capture_depth > 0 {
                        // Move content to pending_fd_output for compound
                        // redirect routing (e.g. `echo msg 1>&3` inside
                        // `{ ... } 3>&1 >file`).
                        self.append_pending_fd_output(dst, &data);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn disabled_redirect_error(&self, redirects: &[Redirect]) -> Option<String> {
        for redirect in redirects {
            if !self.shell_features.has_process_substitution()
                && word_has_process_substitution(&redirect.target)
            {
                return Some("bash: process substitution disabled\n".to_string());
            }

            if !self.shell_features.has_file_redirects()
                && matches!(
                    redirect.kind,
                    RedirectKind::Output
                        | RedirectKind::Clobber
                        | RedirectKind::Append
                        | RedirectKind::Input
                        | RedirectKind::OutputBoth
                )
                && !word_is_literal_dev_null(&redirect.target)
            {
                return Some(format!(
                    "bash: {}: filesystem redirection disabled\n",
                    redirect_target_label(&redirect.target)
                ));
            }
        }
        None
    }

    /// Apply redirections using an fd-table model for correct left-to-right
    /// ordering when DupOutput and file redirects are mixed (e.g. `2>&1 >file`).
    pub(super) async fn apply_redirections_fd_table(
        &mut self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Build fd table: fd1 = stdout pipe, fd2 = stderr pipe
        let mut fd1 = FdTarget::Stdout;
        let mut fd2 = FdTarget::Stderr;
        self.pending_fd_targets.clear();

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output | RedirectKind::Clobber => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    if self.rejects_directory_target(&target_path, &mut result) {
                        return Ok(result);
                    }
                    let path = self.resolve_path(&target_path);
                    if let Some(target_fd) = dev_fd_alias(&path) {
                        let src_fd = redirect.fd.unwrap_or(1);
                        self.dup_output_fd_table(src_fd, target_fd, &mut fd1, &mut fd2);
                        continue;
                    }

                    if redirect.kind == RedirectKind::Output
                        && self.scoped.variables.get("SHOPT_C").map(|v| v.as_str()) == Some("1")
                        && !is_dev_null(&path)
                        && self.fs.stat(&path).await.is_ok()
                    {
                        result.stdout = crate::StreamData::new();
                        result.stderr = self
                            .redirect_error(&target_path, "cannot overwrite existing file")
                            .into();
                        result.exit_code = 1;
                        self.clear_pending_fd_redirect_state();
                        return Ok(result);
                    }

                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::WriteFile(path, target_path)
                    };
                    match redirect.fd {
                        Some(2) => fd2 = target,
                        Some(n) if n >= 3 => self.pending_fd_targets.push((n, target)),
                        _ => fd1 = target,
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    if self.rejects_directory_target(&target_path, &mut result) {
                        return Ok(result);
                    }
                    let path = self.resolve_path(&target_path);
                    if let Some(target_fd) = dev_fd_alias(&path) {
                        let src_fd = redirect.fd.unwrap_or(1);
                        self.dup_output_fd_table(src_fd, target_fd, &mut fd1, &mut fd2);
                        continue;
                    }
                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::AppendFile(path, target_path)
                    };
                    match redirect.fd {
                        Some(2) => fd2 = target,
                        Some(n) if n >= 3 => self.pending_fd_targets.push((n, target)),
                        _ => fd1 = target,
                    }
                }
                RedirectKind::OutputBoth => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if let Some(target_fd) = dev_fd_alias(&path) {
                        self.dup_output_fd_table(1, target_fd, &mut fd1, &mut fd2);
                        self.dup_output_fd_table(2, 1, &mut fd1, &mut fd2);
                        continue;
                    }
                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::WriteFile(path, target_path)
                    };
                    fd1 = target.clone();
                    fd2 = target;
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let target_fd: i32 = target.parse().unwrap_or(1);
                    let src_fd = redirect.fd.unwrap_or(1);
                    self.dup_output_fd_table(src_fd, target_fd, &mut fd1, &mut fd2);
                }
                RedirectKind::Input
                | RedirectKind::HereString
                | RedirectKind::HereDoc
                | RedirectKind::HereDocStrip
                | RedirectKind::DupInput => {}
            }
        }

        // Route stdout/stderr/fd3+ to their targets (non-async to avoid state machine bloat)
        let orig_stdout = std::mem::take(&mut result.stdout);
        let orig_stderr = std::mem::take(&mut result.stderr);
        let (new_stdout, mut new_stderr, file_writes) = route_fd_table_content(
            &orig_stdout,
            &orig_stderr,
            &fd1,
            &fd2,
            &self.pending_fd_targets,
            &self.pending_fd_output,
        );
        self.clear_pending_fd_redirect_state();

        // Write files
        for (path, (content, is_append, display_path)) in &file_writes {
            let write_result = if *is_append {
                self.fs.append_file(path, content.as_bytes()).await
            } else {
                self.fs.write_file(path, content.as_bytes()).await
            };
            if let Err(e) = write_result {
                new_stderr = self
                    .redirect_error(display_path, &io_error_reason(&e))
                    .into();
                result.exit_code = 1;
                result.stdout = new_stdout;
                result.stderr = new_stderr;
                return Ok(result);
            }
        }

        result.stdout = new_stdout;
        result.stderr = new_stderr;
        Ok(result)
    }

    /// `src_fd>&target_fd` on the fd-table redirect path.
    fn dup_output_fd_table(
        &mut self,
        src_fd: i32,
        target_fd: i32,
        fd1: &mut FdTarget,
        fd2: &mut FdTarget,
    ) {
        // Look up exec_fd_table for persistent fd targets (fd 1/2: see
        // `dup_output_fast`).
        if target_fd >= 3
            && let Some(exec_target) = self.exec_fd_table.get(&target_fd).cloned()
        {
            match src_fd {
                2 => *fd2 = exec_target,
                _ => *fd1 = exec_target,
            }
        } else {
            // Resolve target from current fd table state
            let resolved = match target_fd {
                1 => Some(fd1.clone()),
                2 => Some(fd2.clone()),
                _ => None,
            };
            if let Some(target) = resolved {
                match src_fd {
                    1 => *fd1 = target,
                    2 => *fd2 = target,
                    n if n >= 3 => {
                        // Store fd3+ target for routing pending_fd_output later
                        self.pending_fd_targets.push((n, target));
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Whether any redirect opens a file on fd 3 or above (`3>f`, `9>>lock`).
pub(super) fn has_high_fd_file_redirect(redirects: &[Redirect]) -> bool {
    redirects.iter().any(|r| {
        matches!(
            r.kind,
            RedirectKind::Output | RedirectKind::Clobber | RedirectKind::Append
        ) && r.fd.is_some_and(|fd| fd >= 3)
    })
}
