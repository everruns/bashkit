//! System information builtins (hostname, uname, whoami, id)
//!
//! These builtins return hardcoded sandbox values to prevent
//! information disclosure about the host system.
//!
//! Security rationale: Real system information could be used for:
//! - Fingerprinting the host for targeted attacks
//! - Identifying the environment for escape attempts
//! - Correlating activity across tenants

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Hardcoded sandbox hostname.
/// Using a clearly fake name prevents confusion with real hosts.
pub const SANDBOX_HOSTNAME: &str = "bashkit-sandbox";

/// Hardcoded sandbox username.
pub const SANDBOX_USERNAME: &str = "sandbox";

/// Hardcoded sandbox user ID.
pub const SANDBOX_UID: u32 = 1000;

/// Hardcoded sandbox group ID.
pub const SANDBOX_GID: u32 = 1000;

/// The hostname builtin - returns hardcoded sandbox hostname.
///
/// Real hostname is never exposed to prevent host fingerprinting.
pub struct Hostname;

#[async_trait]
impl Builtin for Hostname {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check for -h or --help
        if ctx.args.first().map(|s| s.as_str()) == Some("-h")
            || ctx.args.first().map(|s| s.as_str()) == Some("--help")
        {
            return Ok(ExecResult::ok(
                "hostname: display sandbox hostname\nUsage: hostname\n",
            ));
        }

        // Ignore any attempts to set hostname
        if !ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "hostname: cannot set hostname in sandbox\n",
                1,
            ));
        }

        Ok(ExecResult::ok(format!("{}\n", SANDBOX_HOSTNAME)))
    }
}

/// The uname builtin - returns hardcoded system information.
///
/// Prevents disclosure of:
/// - Kernel version (could reveal vulnerabilities)
/// - Architecture (could inform exploit selection)
/// - Host machine name
pub struct Uname;

#[async_trait]
impl Builtin for Uname {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut show_all = false;
        let mut show_kernel = false;
        let mut show_nodename = false;
        let mut show_release = false;
        let mut show_version = false;
        let mut show_machine = false;
        let mut show_os = false;

        for arg in ctx.args {
            match arg.as_str() {
                "-a" | "--all" => show_all = true,
                "-s" | "--kernel-name" => show_kernel = true,
                "-n" | "--nodename" => show_nodename = true,
                "-r" | "--kernel-release" => show_release = true,
                "-v" | "--kernel-version" => show_version = true,
                "-m" | "--machine" => show_machine = true,
                "-o" | "--operating-system" => show_os = true,
                "-h" | "--help" => {
                    return Ok(ExecResult::ok(
                        "uname: print sandbox system information\n\
                         Usage: uname [OPTION]...\n\
                         Options:\n\
                         \t-a  print all information\n\
                         \t-s  print kernel name\n\
                         \t-n  print network node hostname\n\
                         \t-r  print kernel release\n\
                         \t-v  print kernel version\n\
                         \t-m  print machine hardware name\n\
                         \t-o  print operating system\n",
                    ));
                }
                _ => {}
            }
        }

        // Default to kernel name if no options
        if !show_all
            && !show_kernel
            && !show_nodename
            && !show_release
            && !show_version
            && !show_machine
            && !show_os
        {
            show_kernel = true;
        }

        let mut parts = Vec::new();

        if show_all || show_kernel {
            parts.push("Linux");
        }
        if show_all || show_nodename {
            parts.push(SANDBOX_HOSTNAME);
        }
        if show_all || show_release {
            parts.push("5.15.0-sandbox");
        }
        if show_all || show_version {
            parts.push("#1 SMP PREEMPT sandbox");
        }
        if show_all || show_machine {
            parts.push("x86_64");
        }
        if show_all || show_os {
            parts.push("GNU/Linux");
        }

        Ok(ExecResult::ok(format!("{}\n", parts.join(" "))))
    }
}

/// The whoami builtin - returns hardcoded sandbox username.
pub struct Whoami;

#[async_trait]
impl Builtin for Whoami {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult::ok(format!("{}\n", SANDBOX_USERNAME)))
    }
}

/// The id builtin - returns hardcoded sandbox user/group IDs.
pub struct Id;

#[async_trait]
impl Builtin for Id {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check for specific flags
        for arg in ctx.args {
            match arg.as_str() {
                "-u" | "--user" => {
                    return Ok(ExecResult::ok(format!("{}\n", SANDBOX_UID)));
                }
                "-g" | "--group" => {
                    return Ok(ExecResult::ok(format!("{}\n", SANDBOX_GID)));
                }
                "-n" | "--name" => {
                    // -n is usually combined with -u or -g
                    continue;
                }
                "-h" | "--help" => {
                    return Ok(ExecResult::ok(
                        "id: print sandbox user and group IDs\n\
                         Usage: id [OPTION]\n\
                         Options:\n\
                         \t-u  print only the effective user ID\n\
                         \t-g  print only the effective group ID\n\
                         \t-n  print a name instead of a number (with -u or -g)\n",
                    ));
                }
                _ => {}
            }
        }

        // Check for -un or -gn combinations
        let args_str: String = ctx.args.iter().map(|s| s.as_str()).collect();
        if args_str.contains('u') && args_str.contains('n') {
            return Ok(ExecResult::ok(format!("{}\n", SANDBOX_USERNAME)));
        }
        if args_str.contains('g') && args_str.contains('n') {
            return Ok(ExecResult::ok(format!("{}\n", SANDBOX_USERNAME)));
        }

        // Default output format
        Ok(ExecResult::ok(format!(
            "uid={}({}) gid={}({}) groups={}({})\n",
            SANDBOX_UID,
            SANDBOX_USERNAME,
            SANDBOX_GID,
            SANDBOX_USERNAME,
            SANDBOX_GID,
            SANDBOX_USERNAME
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_builtin<B: Builtin>(builtin: &B, args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
        };

        builtin.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_hostname_returns_sandbox() {
        let result = run_builtin(&Hostname, &[]).await;
        assert_eq!(result.stdout, "bashkit-sandbox\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_hostname_cannot_set() {
        let result = run_builtin(&Hostname, &["evil.com"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("cannot set"));
    }

    #[tokio::test]
    async fn test_uname_default() {
        let result = run_builtin(&Uname, &[]).await;
        assert_eq!(result.stdout, "Linux\n");
    }

    #[tokio::test]
    async fn test_uname_all() {
        let result = run_builtin(&Uname, &["-a"]).await;
        assert!(result.stdout.contains("Linux"));
        assert!(result.stdout.contains("bashkit-sandbox"));
        assert!(result.stdout.contains("x86_64"));
    }

    #[tokio::test]
    async fn test_uname_nodename() {
        let result = run_builtin(&Uname, &["-n"]).await;
        assert_eq!(result.stdout, "bashkit-sandbox\n");
    }

    #[tokio::test]
    async fn test_whoami() {
        let result = run_builtin(&Whoami, &[]).await;
        assert_eq!(result.stdout, "sandbox\n");
    }

    #[tokio::test]
    async fn test_id_default() {
        let result = run_builtin(&Id, &[]).await;
        assert!(result.stdout.contains("uid=1000"));
        assert!(result.stdout.contains("gid=1000"));
        assert!(result.stdout.contains("sandbox"));
    }

    #[tokio::test]
    async fn test_id_user() {
        let result = run_builtin(&Id, &["-u"]).await;
        assert_eq!(result.stdout, "1000\n");
    }

    #[tokio::test]
    async fn test_id_group() {
        let result = run_builtin(&Id, &["-g"]).await;
        assert_eq!(result.stdout, "1000\n");
    }
}
