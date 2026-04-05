//! SSH, SCP, and SFTP builtins.
//!
//! Provides remote command execution and file transfer via SSH.
//! Requires the `ssh` feature and configuration via `Bash::builder().ssh()`.
//!
//! # Security
//!
//! - Host validated against allowlist before every operation (TM-SSH-001)
//! - Keys read from VFS only, never host filesystem (TM-SSH-002)
//! - Response size enforced by SshClient (TM-SSH-004)

use async_trait::async_trait;

use super::{Context, resolve_path};
use crate::interpreter::ExecResult;

// ── SSH builtin ──────────────────────────────────────────────────────────

/// SSH builtin: execute commands on remote hosts.
///
/// # Usage
///
/// ```text
/// ssh [options] [user@]host [command...]
/// ```
///
/// # Options
///
/// - `-p port` — Remote port (default: 22)
/// - `-i keyfile` — Identity file (private key from VFS)
/// - `-o option` — Ignored (compatibility)
/// - `-q` — Quiet mode (suppress warnings)
/// - `-v` — Verbose mode
pub struct Ssh;

#[async_trait]
impl super::Builtin for Ssh {
    async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
        #[cfg(feature = "ssh")]
        {
            if let Some(ssh_client) = ctx.ssh_client {
                return execute_ssh(ctx, ssh_client).await;
            }
        }

        // Suppress unused variable warning when feature is disabled
        let _ = &ctx;

        Ok(ExecResult::err(
            "ssh: not configured\n\
             Note: SSH requires the 'ssh' feature and configuration via Bash::builder().ssh()\n"
                .to_string(),
            1,
        ))
    }
}

#[cfg(feature = "ssh")]
async fn execute_ssh(
    ctx: Context<'_>,
    ssh_client: &crate::ssh::SshClient,
) -> crate::Result<ExecResult> {
    use crate::ssh::SshTarget;

    let mut port: Option<u16> = None;
    let mut identity_file: Option<String> = None;
    let mut quiet = false;
    let mut user_host: Option<String> = None;
    let mut command_args: Vec<String> = Vec::new();
    let mut parsing_options = true;

    let mut i = 0;
    while i < ctx.args.len() {
        let arg = &ctx.args[i];

        if parsing_options && arg.starts_with('-') {
            match arg.as_str() {
                "-p" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "ssh: option requires an argument -- 'p'\n".to_string(),
                            1,
                        ));
                    }
                    port = Some(ctx.args[i].parse::<u16>().map_err(|_| {
                        crate::Error::Execution(format!("ssh: bad port '{}'\n", ctx.args[i]))
                    })?);
                }
                "-i" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "ssh: option requires an argument -- 'i'\n".to_string(),
                            1,
                        ));
                    }
                    identity_file = Some(ctx.args[i].clone());
                }
                "-o" => {
                    // Skip option=value (compatibility)
                    i += 1;
                }
                "-q" => quiet = true,
                "-v" => {} // verbose: no-op for now
                "--" => {
                    parsing_options = false;
                }
                _ => {
                    // Unknown option — treat as host if no host yet
                    if user_host.is_none() {
                        user_host = Some(arg.clone());
                        parsing_options = false;
                    } else {
                        command_args.push(arg.clone());
                    }
                }
            }
        } else if user_host.is_none() {
            user_host = Some(arg.clone());
            parsing_options = false;
        } else {
            command_args.push(arg.clone());
        }
        i += 1;
    }

    let user_host = match user_host {
        Some(uh) => uh,
        None => {
            return Ok(ExecResult::err(
                "usage: ssh [options] [user@]host [command...]\n".to_string(),
                1,
            ));
        }
    };

    // Parse user@host
    let (user, host) = parse_user_host(&user_host, ssh_client.config());

    let port = port.unwrap_or(ssh_client.config().default_port);

    // Read identity file from VFS if specified, else fall back to config key
    let private_key = if let Some(ref key_path) = identity_file {
        let abs = resolve_path(ctx.cwd, key_path);
        let content = ctx
            .fs
            .read_file(&abs)
            .await
            .map_err(|e| crate::Error::Execution(format!("ssh: {}: {}\n", key_path, e)))?;
        Some(String::from_utf8_lossy(&content).into_owned())
    } else {
        ssh_client.config().default_private_key.clone()
    };

    // Fall back to config default password when no key is provided
    let password = if private_key.is_none() {
        ssh_client.config().default_password.clone()
    } else {
        None
    };

    let target = SshTarget {
        host: host.clone(),
        port,
        user: user.clone(),
        private_key,
        password,
    };

    if command_args.is_empty() {
        // No command: check if there's stdin (heredoc mode)
        if let Some(stdin) = ctx.stdin {
            if stdin.trim().is_empty() {
                // Empty stdin + no command → open shell session
                match ssh_client.shell(&target).await {
                    Ok(output) => Ok(build_result(output, quiet)),
                    Err(e) => Ok(ExecResult::err(format!("ssh: {}\n", e), 255)),
                }
            } else {
                // Execute stdin as remote command
                match ssh_client.exec(&target, stdin.trim()).await {
                    Ok(output) => Ok(build_result(output, quiet)),
                    Err(e) => Ok(ExecResult::err(format!("ssh: {}\n", e), 255)),
                }
            }
        } else {
            // No command, no stdin → open shell session
            match ssh_client.shell(&target).await {
                Ok(output) => Ok(build_result(output, quiet)),
                Err(e) => Ok(ExecResult::err(format!("ssh: {}\n", e), 255)),
            }
        }
    } else {
        // Execute remote command
        let command = command_args.join(" ");
        match ssh_client.exec(&target, &command).await {
            Ok(output) => Ok(build_result(output, quiet)),
            Err(e) => Ok(ExecResult::err(format!("ssh: {}\n", e), 255)),
        }
    }
}

// ── SCP builtin ──────────────────────────────────────────────────────────

/// SCP builtin: copy files to/from remote hosts.
///
/// # Usage
///
/// ```text
/// scp [options] source... target
/// scp local_file [user@]host:remote_path
/// scp [user@]host:remote_path local_file
/// ```
///
/// # Options
///
/// - `-P port` — Remote port
/// - `-i keyfile` — Identity file
/// - `-q` — Quiet mode
/// - `-r` — Recursive (directories)
pub struct Scp;

#[async_trait]
impl super::Builtin for Scp {
    async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
        #[cfg(feature = "ssh")]
        {
            if let Some(ssh_client) = ctx.ssh_client {
                return execute_scp(ctx, ssh_client).await;
            }
        }

        let _ = &ctx;

        Ok(ExecResult::err(
            "scp: not configured\n\
             Note: SCP requires the 'ssh' feature and configuration via Bash::builder().ssh()\n"
                .to_string(),
            1,
        ))
    }
}

#[cfg(feature = "ssh")]
async fn execute_scp(
    ctx: Context<'_>,
    ssh_client: &crate::ssh::SshClient,
) -> crate::Result<ExecResult> {
    use crate::ssh::SshTarget;

    let mut port: Option<u16> = None;
    let mut identity_file: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < ctx.args.len() {
        let arg = &ctx.args[i];
        match arg.as_str() {
            "-P" => {
                i += 1;
                if i >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "scp: option requires an argument -- 'P'\n".to_string(),
                        1,
                    ));
                }
                port = Some(ctx.args[i].parse::<u16>().map_err(|_| {
                    crate::Error::Execution(format!("scp: bad port '{}'\n", ctx.args[i]))
                })?);
            }
            "-i" => {
                i += 1;
                if i >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "scp: option requires an argument -- 'i'\n".to_string(),
                        1,
                    ));
                }
                identity_file = Some(ctx.args[i].clone());
            }
            "-q" | "-r" => {} // quiet/recursive: accept but no-op for now
            _ => positional.push(arg.clone()),
        }
        i += 1;
    }

    if positional.len() < 2 {
        return Ok(ExecResult::err(
            "usage: scp [options] source target\n".to_string(),
            1,
        ));
    }

    let source = &positional[0];
    let target_str = &positional[1];

    // Read identity file from VFS if specified, else fall back to config key
    let private_key = if let Some(ref key_path) = identity_file {
        let abs = resolve_path(ctx.cwd, key_path);
        let content = ctx
            .fs
            .read_file(&abs)
            .await
            .map_err(|e| crate::Error::Execution(format!("scp: {}: {}\n", key_path, e)))?;
        Some(String::from_utf8_lossy(&content).into_owned())
    } else {
        ssh_client.config().default_private_key.clone()
    };

    let port = port.unwrap_or(ssh_client.config().default_port);
    let password = if private_key.is_none() {
        ssh_client.config().default_password.clone()
    } else {
        None
    };

    // Determine direction: upload or download
    if let Some((remote_spec, remote_path)) = parse_remote_path(target_str) {
        // Upload: scp local_file user@host:remote_path
        let (user, host) = parse_user_host(&remote_spec, ssh_client.config());
        let local_path = resolve_path(ctx.cwd, source);
        let content = ctx
            .fs
            .read_file(&local_path)
            .await
            .map_err(|e| crate::Error::Execution(format!("scp: {}: {}\n", source, e)))?;

        let ssh_target = SshTarget {
            host,
            port,
            user,
            private_key,
            password: password.clone(),
        };

        match ssh_client
            .upload(&ssh_target, &remote_path, &content, 0o644)
            .await
        {
            Ok(()) => Ok(ExecResult::ok(String::new())),
            Err(e) => Ok(ExecResult::err(format!("scp: {}\n", e), 1)),
        }
    } else if let Some((remote_spec, remote_path)) = parse_remote_path(source) {
        // Download: scp user@host:remote_path local_file
        let (user, host) = parse_user_host(&remote_spec, ssh_client.config());
        let local_path = resolve_path(ctx.cwd, target_str);

        let ssh_target = SshTarget {
            host,
            port,
            user,
            private_key,
            password,
        };

        match ssh_client.download(&ssh_target, &remote_path).await {
            Ok(data) => {
                ctx.fs.write_file(&local_path, &data).await.map_err(|e| {
                    crate::Error::Execution(format!("scp: {}: {}\n", target_str, e))
                })?;
                Ok(ExecResult::ok(String::new()))
            }
            Err(e) => Ok(ExecResult::err(format!("scp: {}\n", e), 1)),
        }
    } else {
        Ok(ExecResult::err(
            "scp: no remote host specified\n\
             usage: scp local_file [user@]host:path\n\
                    scp [user@]host:path local_file\n"
                .to_string(),
            1,
        ))
    }
}

// ── SFTP builtin ─────────────────────────────────────────────────────────

/// SFTP builtin: file transfer via SSH.
///
/// In bashkit, SFTP works in non-interactive mode only (pipe/heredoc).
///
/// # Usage
///
/// ```text
/// sftp [options] [user@]host <<EOF
/// put local_file remote_path
/// get remote_path local_file
/// EOF
/// ```
///
/// # Supported Commands
///
/// - `put local_file remote_path` — Upload file
/// - `get remote_path local_file` — Download file
/// - `ls [path]` — List remote directory (via ssh ls)
pub struct Sftp;

#[async_trait]
impl super::Builtin for Sftp {
    async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
        #[cfg(feature = "ssh")]
        {
            if let Some(ssh_client) = ctx.ssh_client {
                return execute_sftp(ctx, ssh_client).await;
            }
        }

        let _ = &ctx;

        Ok(ExecResult::err(
            "sftp: not configured\n\
             Note: SFTP requires the 'ssh' feature and configuration via Bash::builder().ssh()\n"
                .to_string(),
            1,
        ))
    }
}

#[cfg(feature = "ssh")]
async fn execute_sftp(
    ctx: Context<'_>,
    ssh_client: &crate::ssh::SshClient,
) -> crate::Result<ExecResult> {
    use crate::ssh::SshTarget;

    let mut port: Option<u16> = None;
    let mut identity_file: Option<String> = None;
    let mut user_host: Option<String> = None;

    let mut i = 0;
    while i < ctx.args.len() {
        let arg = &ctx.args[i];
        match arg.as_str() {
            "-P" => {
                i += 1;
                if i >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "sftp: option requires an argument -- 'P'\n".to_string(),
                        1,
                    ));
                }
                port = Some(ctx.args[i].parse::<u16>().map_err(|_| {
                    crate::Error::Execution(format!("sftp: bad port '{}'\n", ctx.args[i]))
                })?);
            }
            "-i" => {
                i += 1;
                if i >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "sftp: option requires an argument -- 'i'\n".to_string(),
                        1,
                    ));
                }
                identity_file = Some(ctx.args[i].clone());
            }
            _ => {
                if user_host.is_none() {
                    user_host = Some(arg.clone());
                }
            }
        }
        i += 1;
    }

    let user_host = match user_host {
        Some(uh) => uh,
        None => {
            return Ok(ExecResult::err(
                "usage: sftp [options] [user@]host\n".to_string(),
                1,
            ));
        }
    };

    let stdin = match ctx.stdin {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            return Ok(ExecResult::err(
                "sftp: interactive mode not supported\n\
                 hint: use heredoc or pipe commands to sftp\n"
                    .to_string(),
                1,
            ));
        }
    };

    let (user, host) = parse_user_host(&user_host, ssh_client.config());
    let port = port.unwrap_or(ssh_client.config().default_port);

    let private_key = if let Some(ref key_path) = identity_file {
        let abs = resolve_path(ctx.cwd, key_path);
        let content = ctx
            .fs
            .read_file(&abs)
            .await
            .map_err(|e| crate::Error::Execution(format!("sftp: {}: {}\n", key_path, e)))?;
        Some(String::from_utf8_lossy(&content).into_owned())
    } else {
        ssh_client.config().default_private_key.clone()
    };

    let password = if private_key.is_none() {
        ssh_client.config().default_password.clone()
    } else {
        None
    };

    let target = SshTarget {
        host,
        port,
        user,
        private_key,
        password,
    };

    let mut output = String::new();
    let mut last_exit = 0;

    for line in stdin.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        match parts.first().copied() {
            Some("put") => {
                if parts.len() < 3 {
                    output.push_str("sftp: put requires local_file and remote_path\n");
                    last_exit = 1;
                    continue;
                }
                let local_path = resolve_path(ctx.cwd, parts[1]);
                let content = match ctx.fs.read_file(&local_path).await {
                    Ok(c) => c,
                    Err(e) => {
                        output.push_str(&format!("sftp: {}: {}\n", parts[1], e));
                        last_exit = 1;
                        continue;
                    }
                };
                match ssh_client.upload(&target, parts[2], &content, 0o644).await {
                    Ok(()) => {}
                    Err(e) => {
                        output.push_str(&format!("sftp: put: {}\n", e));
                        last_exit = 1;
                    }
                }
            }
            Some("get") => {
                if parts.len() < 3 {
                    output.push_str("sftp: get requires remote_path and local_file\n");
                    last_exit = 1;
                    continue;
                }
                match ssh_client.download(&target, parts[1]).await {
                    Ok(data) => {
                        let local_path = resolve_path(ctx.cwd, parts[2]);
                        if let Err(e) = ctx.fs.write_file(&local_path, &data).await {
                            output.push_str(&format!("sftp: {}: {}\n", parts[2], e));
                            last_exit = 1;
                        }
                    }
                    Err(e) => {
                        output.push_str(&format!("sftp: get: {}\n", e));
                        last_exit = 1;
                    }
                }
            }
            Some("ls") => {
                let path = parts.get(1).copied().unwrap_or(".");
                let cmd = format!("ls -la {}", path);
                match ssh_client.exec(&target, &cmd).await {
                    Ok(result) => {
                        output.push_str(&result.stdout);
                        if !result.stderr.is_empty() {
                            output.push_str(&result.stderr);
                        }
                    }
                    Err(e) => {
                        output.push_str(&format!("sftp: ls: {}\n", e));
                        last_exit = 1;
                    }
                }
            }
            Some(cmd) => {
                output.push_str(&format!("sftp: unsupported command '{}'\n", cmd));
                last_exit = 1;
            }
            None => {}
        }
    }

    if last_exit == 0 {
        Ok(ExecResult::ok(output))
    } else {
        Ok(ExecResult::err(output, last_exit))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Parse `user@host` into (user, host). Falls back to config default user.
#[cfg(feature = "ssh")]
fn parse_user_host(spec: &str, config: &crate::ssh::SshConfig) -> (String, String) {
    if let Some(at_pos) = spec.find('@') {
        let user = spec[..at_pos].to_string();
        let host = spec[at_pos + 1..].to_string();
        (user, host)
    } else {
        let user = config
            .default_user
            .clone()
            .unwrap_or_else(|| "root".to_string());
        (user, spec.to_string())
    }
}

/// Parse `[user@]host:path` into (user_host_part, path).
/// Returns None if there's no `:` separator.
#[cfg(feature = "ssh")]
fn parse_remote_path(spec: &str) -> Option<(String, String)> {
    // Don't match Windows-style paths like C:\...
    // A remote spec has : after hostname, not after a single letter
    if let Some(colon_pos) = spec.find(':') {
        // Ensure it's not a drive letter (single char before colon)
        if colon_pos > 1 || !spec.as_bytes()[0].is_ascii_alphabetic() {
            let remote_spec = spec[..colon_pos].to_string();
            let path = spec[colon_pos + 1..].to_string();
            return Some((remote_spec, path));
        }
    }
    None
}

/// Build an ExecResult from SSH output.
#[cfg(feature = "ssh")]
fn build_result(output: crate::ssh::SshOutput, _quiet: bool) -> ExecResult {
    if output.exit_code == 0 {
        let mut result = ExecResult::ok(output.stdout);
        if !output.stderr.is_empty() {
            result.stderr = output.stderr;
        }
        result
    } else {
        let mut result = ExecResult::err(output.stdout, output.exit_code);
        if !output.stderr.is_empty() {
            result.stderr = output.stderr;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "ssh")]
    use super::*;

    #[test]
    #[cfg(feature = "ssh")]
    fn test_parse_user_host_with_user() {
        let config = crate::ssh::SshConfig::new();
        let (user, host) = parse_user_host("deploy@db.supabase.co", &config);
        assert_eq!(user, "deploy");
        assert_eq!(host, "db.supabase.co");
    }

    #[test]
    #[cfg(feature = "ssh")]
    fn test_parse_user_host_without_user() {
        let config = crate::ssh::SshConfig::new().default_user("admin");
        let (user, host) = parse_user_host("db.supabase.co", &config);
        assert_eq!(user, "admin");
        assert_eq!(host, "db.supabase.co");
    }

    #[test]
    #[cfg(feature = "ssh")]
    fn test_parse_user_host_no_default() {
        let config = crate::ssh::SshConfig::new();
        let (user, host) = parse_user_host("db.supabase.co", &config);
        assert_eq!(user, "root");
        assert_eq!(host, "db.supabase.co");
    }

    #[test]
    #[cfg(feature = "ssh")]
    fn test_parse_remote_path() {
        assert_eq!(
            parse_remote_path("user@host:/tmp/file"),
            Some(("user@host".to_string(), "/tmp/file".to_string()))
        );
        assert_eq!(
            parse_remote_path("host:file.txt"),
            Some(("host".to_string(), "file.txt".to_string()))
        );
        assert_eq!(parse_remote_path("local_file.txt"), None);
    }
}
