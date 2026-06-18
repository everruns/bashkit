//! Redirection handling (`>`, `>>`, `<`, `<<<`, fd duplication, fd table).
//!
//! Split out of interpreter/mod.rs: input-redirection collection and the
//! output-redirection / fd-routing core. The `FdTarget` enum and
//! `route_fd_table_content` helper stay in the parent module (referenced
//! by interpreter state fields).

use super::*;

impl Interpreter {
    /// Process input redirections (< file, <<< string)
    pub(super) async fn process_input_redirections(
        &mut self,
        existing_stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<Option<String>> {
        let mut stdin = existing_stdin;

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Input => {
                    if self.shell_profile.is_logic_only()
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
                    if is_dev_null(&path) {
                        stdin = Some(String::new()); // EOF
                    } else if self.shell_profile.is_logic_only() {
                        return Err(crate::error::Error::Execution(format!(
                            "bash: {}: filesystem redirection disabled",
                            target_path
                        )));
                    } else {
                        match self.fs.read_file(&path).await {
                            Ok(content) => {
                                stdin = Some(decode_file_bytes_for_path(&path, &content));
                            }
                            Err(e) => {
                                return Err(crate::error::Error::CommandFailure(format!(
                                    "bash: {target_path}: {e}\n"
                                )));
                            }
                        }
                    }
                }
                RedirectKind::HereString => {
                    // <<< string - use the target as stdin content
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(format!("{}\n", content));
                }
                RedirectKind::HereDoc | RedirectKind::HereDocStrip => {
                    // << EOF / <<- EOF - use the heredoc content as stdin
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(content);
                }
                RedirectKind::DupInput => {
                    // <&FD - if FD is a coproc read FD, consume next line
                    let target = self.expand_word(&redirect.target).await?;
                    if let Ok(fd) = target.parse::<i32>()
                        && let Some(buf) = self.coproc_buffers.get_mut(&fd)
                    {
                        if let Some(line) = buf.pop() {
                            stdin = Some(format!("{}\n", line));
                        } else {
                            stdin = Some(String::new()); // EOF
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
        if let Some(stderr) = self.logic_only_redirect_error(redirects) {
            result.stdout = String::new();
            result.stderr = stderr;
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

        if has_dup_output && has_file_redirect {
            return self.apply_redirections_fd_table(result, redirects).await;
        }

        // --- Fast path: no mixed dup+file redirects ---
        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output | RedirectKind::Clobber => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if is_dev_null(&path) {
                        match redirect.fd {
                            Some(2) => result.stderr = String::new(),
                            _ => {
                                result.stdout = String::new();
                                result.stdout_bytes = None;
                            }
                        }
                    } else {
                        if redirect.kind == RedirectKind::Output
                            && self.variables.get("SHOPT_C").map(|v| v.as_str()) == Some("1")
                            && self.fs.stat(&path).await.is_ok()
                        {
                            result.stdout = String::new();
                            result.stderr =
                                format!("bash: {}: cannot overwrite existing file\n", target_path);
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        match redirect.fd {
                            Some(2) => {
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = String::new();
                            }
                            _ => {
                                let stdout = result
                                    .stdout_bytes
                                    .as_deref()
                                    .unwrap_or(result.stdout.as_bytes());
                                if let Err(e) = self.fs.write_file(&path, stdout).await {
                                    result.stdout = String::new();
                                    result.stdout_bytes = None;
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = String::new();
                                result.stdout_bytes = None;
                            }
                        }
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if is_dev_null(&path) {
                        match redirect.fd {
                            Some(2) => result.stderr = String::new(),
                            _ => {
                                result.stdout = String::new();
                                result.stdout_bytes = None;
                            }
                        }
                    } else {
                        match redirect.fd {
                            Some(2) => {
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = String::new();
                            }
                            _ => {
                                let stdout = result
                                    .stdout_bytes
                                    .as_deref()
                                    .unwrap_or(result.stdout.as_bytes());
                                if let Err(e) = self.fs.append_file(&path, stdout).await {
                                    result.stdout = String::new();
                                    result.stdout_bytes = None;
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = String::new();
                                result.stdout_bytes = None;
                            }
                        }
                    }
                }
                RedirectKind::OutputBoth => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    if is_dev_null(&path) {
                        result.stdout = String::new();
                        result.stdout_bytes = None;
                        result.stderr = String::new();
                    } else {
                        let mut combined = result
                            .stdout_bytes
                            .clone()
                            .unwrap_or_else(|| result.stdout.as_bytes().to_vec());
                        combined.extend_from_slice(result.stderr.as_bytes());
                        if let Err(e) = self.fs.write_file(&path, &combined).await {
                            result.stderr = format!("bash: {}: {}\n", target_path, e);
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        result.stdout = String::new();
                        result.stdout_bytes = None;
                        result.stderr = String::new();
                    }
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_word(&redirect.target).await?;
                    let target_fd: i32 = target.parse().unwrap_or(1);
                    let src_fd = redirect.fd.unwrap_or(1);

                    // Check exec_fd_table for persistent fd targets
                    if let Some(fd_target) = self.exec_fd_table.get(&target_fd).cloned() {
                        let data = if src_fd == 2 {
                            std::mem::take(&mut result.stderr)
                        } else {
                            std::mem::take(&mut result.stdout)
                        };
                        match &fd_target {
                            FdTarget::Stdout => result.stdout.push_str(&data),
                            FdTarget::Stderr => result.stderr.push_str(&data),
                            FdTarget::DevNull => {}
                            FdTarget::WriteFile(path, _) | FdTarget::AppendFile(path, _) => {
                                self.fs.append_file(path, data.as_bytes()).await?;
                            }
                        }
                    } else {
                        match (src_fd, target_fd) {
                            (2, 1) => {
                                result.stdout.push_str(&result.stderr);
                                result.stderr = String::new();
                            }
                            (1, 2) => {
                                result.stderr.push_str(&result.stdout);
                                result.stdout = String::new();
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

    pub(super) fn logic_only_redirect_error(&self, redirects: &[Redirect]) -> Option<String> {
        if !self.shell_profile.is_logic_only() {
            return None;
        }

        for redirect in redirects {
            if word_has_process_substitution(&redirect.target) {
                return Some("bash: process substitution disabled in logic-only shell".to_string());
            }

            if matches!(
                redirect.kind,
                RedirectKind::Output
                    | RedirectKind::Clobber
                    | RedirectKind::Append
                    | RedirectKind::Input
                    | RedirectKind::OutputBoth
            ) && !word_is_literal_dev_null(&redirect.target)
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
                    let path = self.resolve_path(&target_path);

                    if redirect.kind == RedirectKind::Output
                        && self.variables.get("SHOPT_C").map(|v| v.as_str()) == Some("1")
                        && !is_dev_null(&path)
                        && self.fs.stat(&path).await.is_ok()
                    {
                        result.stdout = String::new();
                        result.stderr =
                            format!("bash: {}: cannot overwrite existing file\n", target_path);
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
                        _ => fd1 = target,
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    let target = if is_dev_null(&path) {
                        FdTarget::DevNull
                    } else {
                        FdTarget::AppendFile(path, target_path)
                    };
                    match redirect.fd {
                        Some(2) => fd2 = target,
                        _ => fd1 = target,
                    }
                }
                RedirectKind::OutputBoth => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
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

                    // Look up exec_fd_table for persistent fd targets
                    if let Some(exec_target) = self.exec_fd_table.get(&target_fd).cloned() {
                        match src_fd {
                            2 => fd2 = exec_target,
                            _ => fd1 = exec_target,
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
                                1 => fd1 = target,
                                2 => fd2 = target,
                                n if n >= 3 => {
                                    // Store fd3+ target for routing pending_fd_output later
                                    self.pending_fd_targets.push((n, target));
                                }
                                _ => {}
                            }
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
                new_stderr = format!("bash: {}: {}\n", display_path, e);
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
}
