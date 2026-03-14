//! envsubst builtin command - substitute environment variables in text

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The envsubst builtin command.
///
/// Usage: envsubst [SHELL-FORMAT] < input
///
/// Substitutes `$VAR` and `${VAR}` references with environment variable values.
///
/// Options:
///   -v        List variables found in input
///   SHELL-FORMAT  Only substitute listed variables (e.g. '$HOST $PORT')
pub struct Envsubst;

#[async_trait]
impl Builtin for Envsubst {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut list_vars = false;
        let mut restrict_vars: Option<Vec<String>> = None;

        for arg in ctx.args {
            match arg.as_str() {
                "-v" | "--variables" => list_vars = true,
                s if s.starts_with('$') => {
                    // SHELL-FORMAT: list of vars to substitute
                    let vars: Vec<String> = s
                        .split_whitespace()
                        .map(|v| {
                            v.trim_start_matches('$')
                                .trim_matches(|c| c == '{' || c == '}')
                        })
                        .filter(|v| !v.is_empty())
                        .map(|v| v.to_string())
                        .collect();
                    restrict_vars = Some(vars);
                }
                _ => {}
            }
        }

        let input = ctx.stdin.unwrap_or("");

        if list_vars {
            // List variables found in input
            let vars = find_variables(input);
            let mut output = String::new();
            for var in vars {
                output.push_str(&var);
                output.push('\n');
            }
            return Ok(ExecResult::ok(output));
        }

        let output = substitute(input, ctx.env, ctx.variables, restrict_vars.as_deref());
        Ok(ExecResult::ok(output))
    }
}

fn find_variables(input: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' {
            i += 1;
            if i < chars.len() && chars[i] == '{' {
                // ${VAR}
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if !name.is_empty() && !vars.contains(&name) {
                    vars.push(name);
                }
                if i < chars.len() {
                    i += 1; // skip }
                }
            } else {
                // $VAR
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if !name.is_empty() && !vars.contains(&name) {
                    vars.push(name);
                }
            }
        } else {
            i += 1;
        }
    }

    vars
}

fn substitute(
    input: &str,
    env: &std::collections::HashMap<String, String>,
    variables: &std::collections::HashMap<String, String>,
    restrict: Option<&[String]>,
) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' {
            i += 1;
            if i < chars.len() && chars[i] == '{' {
                // ${VAR}
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if i < chars.len() {
                    i += 1; // skip }
                }
                if should_substitute(&name, restrict) {
                    if let Some(val) = env.get(&name).or_else(|| variables.get(&name)) {
                        output.push_str(val);
                    }
                    // If not found, substitute with empty string
                } else {
                    output.push_str("${");
                    output.push_str(&name);
                    output.push('}');
                }
            } else {
                // $VAR
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if should_substitute(&name, restrict) {
                    if let Some(val) = env.get(&name).or_else(|| variables.get(&name)) {
                        output.push_str(val);
                    }
                } else {
                    output.push('$');
                    output.push_str(&name);
                }
            }
        } else {
            output.push(chars[i]);
            i += 1;
        }
    }

    output
}

fn should_substitute(name: &str, restrict: Option<&[String]>) -> bool {
    match restrict {
        Some(allowed) => allowed.iter().any(|v| v == name),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_envsubst(
        args: &[&str],
        stdin: Option<&str>,
        env: HashMap<String, String>,
    ) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        Envsubst.execute(ctx).await.expect("envsubst failed")
    }

    #[tokio::test]
    async fn test_basic_substitution() {
        let mut env = HashMap::new();
        env.insert("HOST".to_string(), "localhost".to_string());
        env.insert("PORT".to_string(), "8080".to_string());
        let result = run_envsubst(&[], Some("server=$HOST:$PORT"), env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "server=localhost:8080");
    }

    #[tokio::test]
    async fn test_braced_substitution() {
        let mut env = HashMap::new();
        env.insert("NAME".to_string(), "world".to_string());
        let result = run_envsubst(&[], Some("hello ${NAME}!"), env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello world!");
    }

    #[tokio::test]
    async fn test_missing_var_becomes_empty() {
        let env = HashMap::new();
        let result = run_envsubst(&[], Some("value=$MISSING"), env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "value=");
    }

    #[tokio::test]
    async fn test_list_variables() {
        let env = HashMap::new();
        let result = run_envsubst(&["-v"], Some("$HOST and ${PORT} and $DB"), env).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("HOST"));
        assert!(result.stdout.contains("PORT"));
        assert!(result.stdout.contains("DB"));
    }

    #[tokio::test]
    async fn test_restrict_variables() {
        let mut env = HashMap::new();
        env.insert("HOST".to_string(), "localhost".to_string());
        env.insert("PORT".to_string(), "8080".to_string());
        let result = run_envsubst(&["$HOST"], Some("$HOST:$PORT"), env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "localhost:$PORT");
    }

    #[tokio::test]
    async fn test_no_vars_passthrough() {
        let env = HashMap::new();
        let result = run_envsubst(&[], Some("no variables here"), env).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "no variables here");
    }
}
