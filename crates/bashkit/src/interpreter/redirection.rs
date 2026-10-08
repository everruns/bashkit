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
        // Readable fds this command's own redirects open (`4<<E <&4`).
        let mut own_fds: Vec<(i32, crate::StreamData)> = Vec::new();

        for redirect in redirects {
            // Fd N >= 1 for an input-side redirect (`3<f`, `4<<<w`, `5<>f`).
            let high_fd = redirect.fd.filter(|&fd| fd != 0);
            match redirect.kind {
                RedirectKind::ReadWrite => {
                    // `<>`: open (creating) the file read-write.
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    let content = if is_dev_null(&path) {
                        crate::StreamData::new()
                    } else if !self.shell_features.has_file_redirects() {
                        return Err(crate::error::Error::Execution(format!(
                            "bash: {}: filesystem redirection disabled",
                            target_path
                        )));
                    } else {
                        let opened = match self.fs.append_file(&path, b"").await {
                            Ok(()) => self.fs.read_file(&path).await,
                            Err(e) => Err(e),
                        };
                        match opened {
                            Ok(content) => content.into(),
                            Err(e) => {
                                return Err(crate::error::Error::CommandFailure(format!(
                                    "bash: {target_path}: {}\n",
                                    io_error_reason(&e)
                                )));
                            }
                        }
                    };
                    match high_fd {
                        Some(fd) => own_fds.push((fd, content)),
                        None => stdin = Some(content),
                    }
                }
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
                                let content: crate::StreamData = content.into();
                                if let Some(fd) = high_fd {
                                    own_fds.push((fd, content.clone()));
                                }
                                // WTF: `3<file` also feeds stdin; loops like
                                // `while read -u 3 ...; done 3<file` rely on
                                // it until per-command fds are modelled.
                                stdin = Some(content);
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
                    let content: crate::StreamData = format!("{}\n", content).into();
                    match high_fd {
                        Some(fd) => own_fds.push((fd, content)),
                        None => stdin = Some(content),
                    }
                }
                RedirectKind::HereDoc | RedirectKind::HereDocStrip => {
                    // << EOF / <<- EOF - use the heredoc content as stdin;
                    // `3<<EOF` feeds fd 3.
                    let content: crate::StreamData =
                        self.expand_word(&redirect.target).await?.into();
                    match high_fd {
                        Some(fd) => own_fds.push((fd, content)),
                        None => stdin = Some(content),
                    }
                }
                RedirectKind::DupInput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let Ok(src) = target.parse::<i32>() else {
                        continue;
                    };
                    // `<&4` after this command's own `4<<E`.
                    if let Some((_, content)) = own_fds.iter().rev().find(|(fd, _)| *fd == src) {
                        let content = content.clone();
                        match high_fd {
                            Some(fd) => own_fds.push((fd, content)),
                            None => stdin = Some(content),
                        }
                        continue;
                    }
                    if high_fd.is_some() {
                        continue;
                    }
                    // <&FD - if FD is a coproc read FD, consume next line
                    if let Some(buf) = self.coproc_buffers.get_mut(&src) {
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

        // `>&N` with fd N not open: bash reports it and does not run the
        // command, so none of its output may show.
        for redirect in redirects {
            // Side-effect-free targets only (`>&3`, `>&$fd`): the word is
            // expanded again when the redirect is applied.
            if redirect.kind != RedirectKind::DupOutput
                || !redirect
                    .target
                    .parts
                    .iter()
                    .all(|p| matches!(p, WordPart::Literal(_) | WordPart::Variable(_)))
            {
                continue;
            }
            let target = self.expand_word(&redirect.target).await?;
            if let Ok(fd) = target.parse::<i32>()
                && fd >= 3
                && !self.fd_is_open(fd)
                && !redirects
                    .iter()
                    .any(|r| r.fd == Some(fd) && !matches!(r.kind, RedirectKind::DupOutput))
            {
                self.clear_pending_fd_redirect_state();
                result.stdout = crate::StreamData::new();
                result.stderr = format!("bash: {target}: Bad file descriptor\n").into();
                result.exit_code = 1;
                return Ok(result);
            }
        }

        // `N>&-` among other redirects (`2>&1 1>&-`) depends on their order,
        // which only the fd-table path tracks.
        let has_close = redirects.len() > 1
            && redirects.iter().any(|r| {
                r.kind == RedirectKind::DupOutput
                    && matches!(r.target.parts.as_slice(), [WordPart::Literal(t)] if t == "-")
            });

        // `N>file` (N>=3) opens fd N without touching stdout; only the
        // fd-table path keeps fd N separate from fd 1.
        if (has_dup_output && has_file_redirect)
            || has_close
            || has_high_fd_file_redirect(redirects)
        {
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
                // `<>` on stdin is input only (see process_input_redirections).
                RedirectKind::ReadWrite if matches!(redirect.fd, None | Some(0)) => {}
                // WTF: `1<>file` appends instead of writing at offset 0.
                RedirectKind::Append | RedirectKind::ReadWrite => {
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
                    if !self.output_both_fast(&mut result, &target_path).await? {
                        return Ok(result);
                    }
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let src_fd = redirect.fd.unwrap_or(1);
                    match DupTarget::parse(&target) {
                        DupTarget::Fd(target_fd) => {
                            self.dup_output_fast(&mut result, src_fd, target_fd).await?;
                        }
                        DupTarget::Close => self.close_output_fast(&mut result, src_fd),
                        // `>&file` / `1>&file`: bash's spelling of `&>file`.
                        DupTarget::File if src_fd == 1 => {
                            if !self.output_both_fast(&mut result, &target).await? {
                                return Ok(result);
                            }
                        }
                        DupTarget::File => {
                            self.ambiguous_dup_target(&mut result, &target);
                            return Ok(result);
                        }
                    }
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

    /// `&>file` on the fast path: both streams go to the file. Returns
    /// false when the file could not be opened (the command's output is
    /// dropped and the error reported, as bash never runs it).
    async fn output_both_fast(
        &mut self,
        result: &mut ExecResult,
        target_path: &str,
    ) -> Result<bool> {
        let path = self.resolve_path(target_path);
        if let Some(target_fd) = dev_fd_alias(&path) {
            // `&> /dev/fd/N` == `>&N 2>&N`, resolved against the
            // original descriptors.
            if target_fd == 2 {
                let mut combined = std::mem::take(&mut result.stdout);
                combined.append(&result.stderr);
                result.stderr = combined;
            } else {
                self.dup_output_fast(result, 1, target_fd).await?;
                self.dup_output_fast(result, 2, target_fd).await?;
            }
        } else if is_dev_null(&path) {
            result.stdout = crate::StreamData::new();
            result.stderr = crate::StreamData::new();
        } else {
            let mut combined = result.stdout.as_bytes().to_vec();
            combined.extend_from_slice(result.stderr.as_bytes());
            if let Err(e) = self.fs.write_file(&path, &combined).await {
                result.stdout = crate::StreamData::new();
                result.stderr = self
                    .redirect_error(target_path, &io_error_reason(&e))
                    .into();
                result.exit_code = 1;
                return Ok(false);
            }
            result.stdout = crate::StreamData::new();
            result.stderr = crate::StreamData::new();
        }
        Ok(true)
    }

    /// `N>&-` on the fast path: fd N is closed for the command. Writes to
    /// a closed stderr vanish; output to a closed stdout fails the command
    /// with a write error, as in bash.
    // WTF: bash names the writing command (`echo: write error`); the
    // redirect is applied after the command ran, so the name is not known.
    fn close_output_fast(&self, result: &mut ExecResult, src_fd: i32) {
        match src_fd {
            1 if !result.stdout.is_empty() => {
                result.stdout = crate::StreamData::new();
                result
                    .stderr
                    .append(&crate::StreamData::from(self.closed_stdout_error()));
                result.exit_code = 1;
            }
            2 => result.stderr = crate::StreamData::new(),
            // Nothing written to stdout; closing fd 3+ for one command
            // routes nothing.
            _ => {}
        }
    }

    fn closed_stdout_error(&self) -> String {
        format!(
            "bash: line {}: write error: Bad file descriptor\n",
            self.current_line
        )
    }

    /// `N>&word` with N other than 1 and a word that is neither a number
    /// nor `-`: bash refuses it without running the command.
    fn ambiguous_dup_target(&self, result: &mut ExecResult, word: &str) {
        result.stdout = crate::StreamData::new();
        result.stderr = self.redirect_error(word, "ambiguous redirect").into();
        result.exit_code = 1;
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
                RedirectKind::ReadWrite if matches!(redirect.fd, None | Some(0)) => {}
                RedirectKind::Append | RedirectKind::ReadWrite => {
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
                    let src_fd = redirect.fd.unwrap_or(1);
                    match DupTarget::parse(&target) {
                        DupTarget::Fd(target_fd) => {
                            self.dup_output_fd_table(src_fd, target_fd, &mut fd1, &mut fd2);
                        }
                        DupTarget::Close => match src_fd {
                            1 => fd1 = FdTarget::Closed,
                            2 => fd2 = FdTarget::Closed,
                            n => self.pending_fd_targets.push((n, FdTarget::Closed)),
                        },
                        // `>&file`: both streams to the file, like `&>file`.
                        DupTarget::File if src_fd == 1 => {
                            if target.ends_with('/') {
                                self.clear_pending_fd_redirect_state();
                                self.rejects_directory_target(&target, &mut result);
                                return Ok(result);
                            }
                            let path = self.resolve_path(&target);
                            let file = if is_dev_null(&path) {
                                FdTarget::DevNull
                            } else {
                                FdTarget::WriteFile(path, target)
                            };
                            fd1 = file.clone();
                            fd2 = file;
                        }
                        DupTarget::File => {
                            self.clear_pending_fd_redirect_state();
                            self.ambiguous_dup_target(&mut result, &target);
                            return Ok(result);
                        }
                    }
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
        let mut orig_stderr = std::mem::take(&mut result.stderr);
        if matches!(fd1, FdTarget::Closed) && !orig_stdout.is_empty() {
            // Writing to the closed stdout fails; the report goes to fd 2.
            let mut report = crate::StreamData::from(self.closed_stdout_error());
            report.append(&orig_stderr);
            orig_stderr = report;
            result.exit_code = 1;
        }
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
            RedirectKind::Output
                | RedirectKind::Clobber
                | RedirectKind::Append
                | RedirectKind::ReadWrite
        ) && r.fd.is_some_and(|fd| fd >= 3)
    })
}

// ---------------------------------------------------------------------------
// Persistent descriptors: `exec` redirections and `{var}` named fds.
//
// Important decisions:
// - The shell's descriptors are virtual, per interpreter (tenant isolation):
//   output fds live in `exec_fd_table`, readable fds in `coproc_buffers`
//   (remaining lines), and dups of stdin in `exec_input_fds`. No host fd is
//   ever touched.
// - `{var}>file` allocates the lowest free fd >= 10 (bash's
//   `fcntl(F_DUPFD, 10)`) and stores it in `var`. bash never undoes a
//   `{var}` redirection, so on any command (not just `exec`) the fd stays
//   open: it is applied here persistently and removed from the command's own
//   redirect list.
// - `>&N` / `<&N` with N closed is `bash: N: Bad file descriptor` (status 1),
//   unless an enclosing command's redirections provide N
//   (`fd_redirect_scope`), whose routing the pending-fd machinery handles.
// - `N<>file` reads the file and appends writes to it. WTF: the shared
//   read/write offset is not modelled; bash writes at the current offset
//   (overwriting from the start of an existing file).
// ---------------------------------------------------------------------------

/// First fd handed out by `{var}` redirections (bash keeps 0-9 for the user).
const NAMED_FD_BASE: i32 = 10;
/// First coproc read fd; coproc pairs are handed out downward from here.
pub(super) const COPROC_FIRST_FD: i32 = 63;

/// Record the fds (>= 3) a command's own redirections open, so commands
/// running inside it may use them (`f 3>&1`, `{ ...; } 3>log`).
pub(super) fn push_redirect_scope_fds(scope: &mut Vec<i32>, redirects: &[Redirect]) {
    for r in redirects {
        if let Some(fd) = r.fd
            && fd >= 3
            && r.fd_var.is_none()
            && !matches!(r.target.parts.as_slice(), [WordPart::Literal(t)] if t == "-")
        {
            scope.push(fd);
        }
    }
}

/// What the word of `N>&word` names once expanded.
enum DupTarget {
    /// A descriptor number: duplicate it.
    Fd(i32),
    /// `-`: close fd N.
    Close,
    /// Anything else: a file (`>&file` means `&>file`).
    File,
}

impl DupTarget {
    fn parse(word: &str) -> Self {
        if word == "-" {
            Self::Close
        } else if !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) {
            word.parse().map_or(Self::File, Self::Fd)
        } else {
            Self::File
        }
    }
}

fn bad_fd_error(word: &str) -> ExecResult {
    ExecResult::err(format!("bash: {word}: Bad file descriptor\n"), 1)
}

fn ambiguous_redirect_error(word: &str) -> ExecResult {
    ExecResult::err(format!("bash: {word}: ambiguous redirect\n"), 1)
}

/// Remaining coproc-buffer lines (stored reversed) as stdin text.
fn buffer_text(lines: &[String]) -> crate::StreamData {
    let mut text = String::new();
    for line in lines.iter().rev() {
        text.push_str(line);
        text.push('\n');
    }
    text.into()
}

/// Lines of `text`, reversed so `pop()` yields the first.
fn reversed_lines(text: &str) -> Vec<String> {
    text.lines().rev().map(str::to_string).collect()
}

impl Interpreter {
    /// Whether `fd` is open in this shell (or provided by an enclosing
    /// command's redirections).
    pub(super) fn fd_is_open(&self, fd: i32) -> bool {
        if matches!(self.exec_fd_table.get(&fd), Some(FdTarget::Closed)) {
            return false;
        }
        (0..=2).contains(&fd)
            || self.exec_fd_table.contains_key(&fd)
            || self.coproc_buffers.contains_key(&fd)
            || self.exec_input_fds.contains(&fd)
            || self.fd_redirect_scope.contains(&fd)
            // coproc pairs, read and write ends
            || (fd > self.coproc_next_fd && fd <= COPROC_FIRST_FD)
    }

    fn close_fd(&mut self, fd: i32) {
        self.exec_fd_table.remove(&fd);
        self.coproc_buffers.remove(&fd);
        self.exec_input_fds.remove(&fd);
    }

    /// `N>&M` / `N<&M` for N >= 3: N becomes a copy of M.
    fn dup_fd(&mut self, n: i32, m: i32) {
        if n == m {
            return;
        }
        let out = (matches!(m, 1 | 2) || self.exec_fd_table.contains_key(&m))
            .then(|| self.exec_fd_alias_target(m));
        let buf = self.coproc_buffers.get(&m).cloned();
        let input = m == 0 || self.exec_input_fds.contains(&m);
        self.close_fd(n);
        if let Some(target) = out {
            self.exec_fd_table.insert(n, target);
        }
        if let Some(buf) = buf {
            self.coproc_buffers.insert(n, buf);
        }
        if input {
            self.exec_input_fds.insert(n);
        }
    }

    /// Lowest free fd >= 10, as bash allocates for `{var}` redirections.
    fn allocate_named_fd(&self) -> Result<i32> {
        let mut fd = NAMED_FD_BASE;
        while self.fd_is_open(fd) {
            fd += 1;
        }
        self.ensure_persistent_fd_capacity(fd)?;
        Ok(fd)
    }

    /// Apply a command's `{var}` redirections persistently and return the
    /// rest, or the failed redirection's result.
    pub(super) async fn apply_named_fd_redirects(
        &mut self,
        redirects: &[Redirect],
    ) -> Result<std::result::Result<Vec<Redirect>, ExecResult>> {
        let mut remaining = Vec::with_capacity(redirects.len());
        for redirect in redirects {
            if redirect.fd_var.is_some() {
                if let Some(err) = self.apply_persistent_redirect(redirect).await? {
                    return Ok(Err(err));
                }
            } else {
                remaining.push(redirect.clone());
            }
        }
        Ok(Ok(remaining))
    }

    /// Apply one redirection to the shell itself (as `exec` does). Returns
    /// the error result when it fails.
    pub(super) async fn apply_persistent_redirect(
        &mut self,
        redirect: &Redirect,
    ) -> Result<Option<ExecResult>> {
        let Some(var) = redirect.fd_var.as_deref() else {
            return self.open_persistent_fd(redirect, redirect.fd).await;
        };
        let dup;
        let mut redirect = redirect;
        if matches!(
            redirect.kind,
            RedirectKind::DupOutput | RedirectKind::DupInput
        ) {
            let target = self.expand_word(&redirect.target).await?;
            if target == "-" {
                // `{var}>&-` closes the fd named by `$var`.
                let value = self.expand_name_or_array_element(var);
                if value.is_empty() {
                    return Ok(Some(ambiguous_redirect_error(var)));
                }
                if let Ok(fd) = value.trim().parse::<i32>()
                    && fd >= 0
                {
                    self.close_fd(fd);
                }
                return Ok(None);
            }
            if target.parse::<i32>().is_err() {
                return Ok(Some(ambiguous_redirect_error(var)));
            }
            dup = Redirect {
                target: Word::quoted_literal(target),
                ..redirect.clone()
            };
            redirect = &dup;
        }
        let fd = self.allocate_named_fd()?;
        if let Some(err) = self.open_persistent_fd(redirect, Some(fd)).await? {
            return Ok(Some(err));
        }
        let base = var.split('[').next().unwrap_or(var);
        let resolved = self.resolve_nameref(base).to_string();
        if self.var_attrs_get(&resolved).contains(VarAttrs::READONLY) {
            self.close_fd(fd);
            return Ok(Some(ExecResult::err(
                format!(
                    "bash: {base}: readonly variable\nbash: {var}: cannot assign fd to variable\n"
                ),
                1,
            )));
        }
        self.set_parameter_expansion_target(var, fd.to_string());
        Ok(None)
    }

    /// Open `redirect` on `fd` (its default fd when `None`) for the rest of
    /// the shell's life.
    async fn open_persistent_fd(
        &mut self,
        redirect: &Redirect,
        fd: Option<i32>,
    ) -> Result<Option<ExecResult>> {
        // `exec 0<f` is `exec <f`.
        let fd = fd.filter(|&n| n != 0 || !redirect_reads(redirect.kind));
        match redirect.kind {
            RedirectKind::Input | RedirectKind::ReadWrite => {
                let target_path = self.expand_word(&redirect.target).await?;
                let path = self.resolve_path(&target_path);
                let read_write = redirect.kind == RedirectKind::ReadWrite;
                if is_dev_null(&path) {
                    match fd {
                        Some(n) => {
                            self.ensure_persistent_fd_capacity(n)?;
                            self.close_fd(n);
                            self.coproc_buffers.insert(n, Vec::new());
                            if read_write {
                                self.exec_fd_table.insert(n, FdTarget::DevNull);
                            }
                        }
                        None => self.pipeline_stdin = Some(crate::StreamData::new()),
                    }
                    return Ok(None);
                }
                if read_write && let Err(e) = self.fs.append_file(&path, b"").await {
                    return Ok(Some(ExecResult::err(
                        format!("bash: {target_path}: {}\n", io_error_reason(&e)),
                        1,
                    )));
                }
                let content = match self.fs.read_file(&path).await {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(Some(ExecResult::err(
                            format!("bash: {target_path}: {}\n", io_error_reason(&e)),
                            1,
                        )));
                    }
                };
                let text = decode_file_bytes_for_path(&path, &content);
                match fd {
                    Some(n) => {
                        self.ensure_persistent_fd_capacity(n)?;
                        self.close_fd(n);
                        self.coproc_buffers.insert(n, reversed_lines(&text));
                        if read_write {
                            self.exec_fd_table
                                .insert(n, FdTarget::AppendFile(path, target_path));
                        }
                    }
                    // exec < file: redirect stdin for subsequent commands
                    None => self.pipeline_stdin = Some(text.into()),
                }
            }
            RedirectKind::HereString | RedirectKind::HereDoc | RedirectKind::HereDocStrip => {
                let mut content = self.expand_word(&redirect.target).await?;
                if redirect.kind == RedirectKind::HereString {
                    content.push('\n');
                }
                match fd {
                    Some(n) => {
                        self.ensure_persistent_fd_capacity(n)?;
                        self.close_fd(n);
                        self.coproc_buffers.insert(n, reversed_lines(&content));
                    }
                    None => self.pipeline_stdin = Some(content.into()),
                }
            }
            RedirectKind::DupInput | RedirectKind::DupOutput => {
                let target = self.expand_word(&redirect.target).await?;
                let default_fd = i32::from(redirect.kind == RedirectKind::DupOutput);
                let n = fd.unwrap_or(default_fd);
                if target == "-" || target == "&-" {
                    // `N>&-` and `N<&-` both close fd N. For 1 and 2 the
                    // entry stays so a later write reports a closed
                    // descriptor instead of reaching the caller.
                    if matches!(n, 1 | 2) {
                        self.ensure_persistent_fd_capacity(n)?;
                        self.exec_fd_table.insert(n, FdTarget::Closed);
                    } else if n >= 3 {
                        self.close_fd(n);
                    }
                    return Ok(None);
                }
                let Ok(m) = target.parse::<i32>() else {
                    if redirect.kind == RedirectKind::DupInput {
                        return Ok(Some(ambiguous_redirect_error(&target)));
                    }
                    // WTF: `exec >&file` (bash: `exec &>file`) is not
                    // implemented; it is ignored.
                    return Ok(None);
                };
                if !self.fd_is_open(m) {
                    return Ok(Some(bad_fd_error(&target)));
                }
                match n {
                    0 => {
                        // `exec <&N`: stdin reads what is left of fd N.
                        if let Some(buf) = self.coproc_buffers.get(&m) {
                            self.pipeline_stdin = Some(buffer_text(buf));
                        }
                    }
                    1 | 2 => {
                        // `exec N>&M` duplicates fd M to fd N
                        let target_entry = self.exec_fd_alias_target(m);
                        self.exec_fd_table.insert(n, target_entry);
                    }
                    _ => {
                        self.ensure_persistent_fd_capacity(n)?;
                        self.dup_fd(n, m);
                    }
                }
            }
            RedirectKind::Output | RedirectKind::Clobber | RedirectKind::Append => {
                let n = fd.unwrap_or(1);
                let append = redirect.kind == RedirectKind::Append;
                let target_path = self.expand_word(&redirect.target).await?;
                let path = self.resolve_path(&target_path);
                self.ensure_persistent_fd_capacity(n)?;
                let entry = if let Some(target_fd) = dev_fd_alias(&path) {
                    self.exec_fd_alias_target(target_fd)
                } else if is_dev_null(&path) {
                    FdTarget::DevNull
                } else {
                    // Truncate (or create) the file on open, like real
                    // `exec >file`; a failed open leaves the fd as it was.
                    let opened = if append {
                        self.fs.append_file(&path, b"").await
                    } else {
                        self.fs.write_file(&path, b"").await
                    };
                    if let Err(e) = opened {
                        return Ok(Some(ExecResult::err(
                            format!("bash: {target_path}: {}\n", io_error_reason(&e)),
                            1,
                        )));
                    }
                    if append {
                        FdTarget::AppendFile(path, target_path)
                    } else {
                        FdTarget::WriteFile(path, target_path)
                    }
                };
                if n >= 3 {
                    self.close_fd(n);
                }
                self.exec_fd_table.insert(n, entry);
            }
            // WTF: `exec &>file` is not implemented; it is ignored.
            RedirectKind::OutputBoth => {}
        }
        Ok(None)
    }
}

/// Whether the redirection's default fd is 0.
fn redirect_reads(kind: RedirectKind) -> bool {
    !matches!(
        kind,
        RedirectKind::Output
            | RedirectKind::Clobber
            | RedirectKind::Append
            | RedirectKind::DupOutput
            | RedirectKind::OutputBoth
    )
}
