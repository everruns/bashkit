//! Curl builtin - transfer data from URLs
//!
//! Note: This builtin requires network feature and proper configuration.
//! Network access is restricted by allowlist for security.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The curl builtin - transfer data from URLs.
///
/// Usage: curl [OPTIONS] URL
///
/// Options:
///   -s, --silent    Silent mode (no progress)
///   -o FILE         Write output to FILE
///   -X METHOD       Specify request method (GET, POST, PUT, DELETE)
///   -d DATA         Send data in POST request
///   -H HEADER       Add header to request
///   -I, --head      Fetch headers only
///
/// Note: Network access requires the 'network' feature and proper
/// URL allowlist configuration. Without configuration, all requests
/// will fail with an access denied error.
pub struct Curl;

#[async_trait]
impl Builtin for Curl {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Parse arguments
        let mut silent = false;
        let mut output_file: Option<String> = None;
        let mut method = "GET".to_string();
        let mut data: Option<String> = None;
        let mut headers: Vec<String> = Vec::new();
        let mut head_only = false;
        let mut url: Option<String> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-s" | "--silent" => silent = true,
                "-I" | "--head" => {
                    head_only = true;
                    method = "HEAD".to_string();
                }
                "-o" => {
                    i += 1;
                    if i < ctx.args.len() {
                        output_file = Some(ctx.args[i].clone());
                    }
                }
                "-X" => {
                    i += 1;
                    if i < ctx.args.len() {
                        method = ctx.args[i].clone().to_uppercase();
                    }
                }
                "-d" | "--data" => {
                    i += 1;
                    if i < ctx.args.len() {
                        data = Some(ctx.args[i].clone());
                        if method == "GET" {
                            method = "POST".to_string();
                        }
                    }
                }
                "-H" | "--header" => {
                    i += 1;
                    if i < ctx.args.len() {
                        headers.push(ctx.args[i].clone());
                    }
                }
                _ if !arg.starts_with('-') => {
                    url = Some(arg.clone());
                }
                _ => {
                    // Ignore unknown options for compatibility
                }
            }
            i += 1;
        }

        // Validate URL
        let url = match url {
            Some(u) => u,
            None => {
                return Ok(ExecResult::err("curl: no URL specified\n".to_string(), 3));
            }
        };

        // For now, return a stub error since network access requires
        // special configuration that's not available in the builtin context.
        // A real implementation would use ctx.http_client if available.

        let _ = (
            silent,
            output_file,
            method,
            data,
            headers,
            head_only,
            ctx.fs,
        );

        Ok(ExecResult::err(
            format!(
                "curl: network access not configured\nURL: {}\n\
                 Note: Network builtins require the 'network' feature and\n\
                 URL allowlist configuration for security.\n",
                url
            ),
            1,
        ))
    }
}

/// The wget builtin - download files from URLs.
///
/// Usage: wget [OPTIONS] URL
///
/// Options:
///   -q, --quiet     Quiet mode
///   -O FILE         Write output to FILE
///
/// Note: Network access requires the 'network' feature and proper
/// URL allowlist configuration.
pub struct Wget;

#[async_trait]
impl Builtin for Wget {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Parse arguments
        let mut quiet = false;
        let mut output_file: Option<String> = None;
        let mut url: Option<String> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-q" | "--quiet" => quiet = true,
                "-O" => {
                    i += 1;
                    if i < ctx.args.len() {
                        output_file = Some(ctx.args[i].clone());
                    }
                }
                _ if !arg.starts_with('-') => {
                    url = Some(arg.clone());
                }
                _ => {
                    // Ignore unknown options
                }
            }
            i += 1;
        }

        // Validate URL
        let url = match url {
            Some(u) => u,
            None => {
                return Ok(ExecResult::err("wget: missing URL\n".to_string(), 1));
            }
        };

        let _ = (quiet, output_file, ctx.fs);

        Ok(ExecResult::err(
            format!(
                "wget: network access not configured\nURL: {}\n\
                 Note: Network builtins require the 'network' feature and\n\
                 URL allowlist configuration for security.\n",
                url
            ),
            1,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_curl(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
        };

        Curl.execute(ctx).await.unwrap()
    }

    async fn run_wget(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
        };

        Wget.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_curl_no_url() {
        let result = run_curl(&[]).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("no URL specified"));
    }

    #[tokio::test]
    async fn test_curl_with_url() {
        let result = run_curl(&["https://example.com"]).await;
        // Should fail gracefully without network config
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network access not configured"));
    }

    #[tokio::test]
    async fn test_wget_no_url() {
        let result = run_wget(&[]).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("missing URL"));
    }

    #[tokio::test]
    async fn test_wget_with_url() {
        let result = run_wget(&["https://example.com"]).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network access not configured"));
    }
}
