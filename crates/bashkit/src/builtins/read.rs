//! read builtin - read a line of input

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_internal_variable};

/// read builtin - read a line of input into variables
pub struct Read;

#[async_trait]
impl Builtin for Read {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Get the input to read from stdin
        let input = match ctx.stdin {
            Some(s) => s.to_string(),
            None => return Ok(ExecResult::err("", 1)),
        };

        // Parse flags
        let mut raw_mode = false; // -r: don't interpret backslashes
        let mut array_mode = false; // -a: read into array
        let mut delimiter = None::<char>; // -d: custom delimiter
        let mut nchars = None::<usize>; // -n: read N chars
        let mut prompt = None::<String>; // -p prompt
        let mut var_args = Vec::new();
        let mut args_iter = ctx.args.iter();
        while let Some(arg) = args_iter.next() {
            if arg.starts_with('-') && arg.len() > 1 {
                let mut chars = arg[1..].chars();
                while let Some(flag) = chars.next() {
                    match flag {
                        'r' => raw_mode = true,
                        'a' => array_mode = true,
                        'd' => {
                            // -d delim: use first char of next arg as delimiter
                            let rest: String = chars.collect();
                            let delim_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("")
                            } else {
                                &rest
                            };
                            delimiter = delim_str.chars().next();
                            break;
                        }
                        'n' => {
                            let rest: String = chars.collect();
                            let n_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("0")
                            } else {
                                &rest
                            };
                            nchars = n_str.parse().ok();
                            break;
                        }
                        'p' => {
                            let rest: String = chars.collect();
                            prompt = Some(if rest.is_empty() {
                                args_iter.next().cloned().unwrap_or_default()
                            } else {
                                rest
                            });
                            break;
                        }
                        't' | 's' | 'u' | 'e' | 'i' => {
                            // -t timeout, -s silent, -u fd: accept and ignore
                            if matches!(flag, 't' | 'u') {
                                let rest: String = chars.collect();
                                if rest.is_empty() {
                                    args_iter.next();
                                }
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                var_args.push(arg.as_str());
            }
        }
        let _ = prompt; // prompt is for interactive use, ignored in non-interactive

        // Extract input based on delimiter or nchars
        let line = if let Some(n) = nchars {
            // -n N: read at most N chars
            input.chars().take(n).collect::<String>()
        } else if let Some(delim) = delimiter {
            // -d delim: read until delimiter
            input.split(delim).next().unwrap_or("").to_string()
        } else if raw_mode {
            // -r: treat backslashes literally
            input.lines().next().unwrap_or("").to_string()
        } else {
            // Without -r: handle backslash line continuation
            let mut result = String::new();
            for l in input.lines() {
                if let Some(stripped) = l.strip_suffix('\\') {
                    result.push_str(stripped);
                } else {
                    result.push_str(l);
                    break;
                }
            }
            result
        };

        // Split line by IFS (default: space, tab, newline)
        // IFS whitespace chars (space, tab, newline) collapse runs and trim.
        // Non-whitespace IFS chars preserve empty fields between consecutive delimiters.
        let ifs = ctx.env.get("IFS").map(|s| s.as_str()).unwrap_or(" \t\n");
        let words: Vec<&str> = if ifs.is_empty() {
            // Empty IFS means no word splitting
            vec![&line]
        } else {
            let ifs_ws: Vec<char> = ifs.chars().filter(|c| " \t\n".contains(*c)).collect();
            let ifs_non_ws: Vec<char> = ifs.chars().filter(|c| !" \t\n".contains(*c)).collect();

            if ifs_non_ws.is_empty() {
                // All IFS chars are whitespace: collapse runs, trim
                line.split(|c: char| ifs.contains(c))
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                // Has non-whitespace delimiters: split on them, trim whitespace from each field
                let mut fields: Vec<&str> = line.split(|c: char| ifs_non_ws.contains(&c)).collect();
                // Trim IFS whitespace from each field
                if !ifs_ws.is_empty() {
                    fields = fields
                        .into_iter()
                        .map(|f| f.trim_matches(|c: char| ifs_ws.contains(&c)))
                        .collect();
                }
                fields
            }
        };

        if array_mode {
            // -a: read all words into array variable
            let arr_name = var_args.first().copied().unwrap_or("REPLY");
            // THREAT[TM-INJ-009]: Block internal variable prefix injection via read -a
            if is_internal_variable(arr_name) {
                return Ok(ExecResult::ok(String::new()));
            }
            let mut result = ExecResult::ok(String::new());
            result.side_effects.push(BuiltinSideEffect::SetArray {
                name: arr_name.to_string(),
                elements: words.iter().map(|w| w.to_string()).collect(),
            });
            return Ok(result);
        }

        // If no variable names given, use REPLY
        let var_names: Vec<&str> = if var_args.is_empty() {
            vec!["REPLY"]
        } else {
            var_args
        };

        // Assign words to variables via side effects (respects local scoping)
        let mut result = ExecResult::ok(String::new());
        for (i, var_name) in var_names.iter().enumerate() {
            // THREAT[TM-INJ-009]: Block internal variable prefix injection via read
            if is_internal_variable(var_name) {
                continue;
            }
            let value = if i == var_names.len() - 1 {
                // Last variable gets all remaining words
                let remaining: Vec<&str> = words.iter().skip(i).copied().collect();
                remaining.join(" ")
            } else if i < words.len() {
                words[i].to_string()
            } else {
                // Not enough words - set to empty
                String::new()
            };
            result.side_effects.push(BuiltinSideEffect::SetVariable {
                name: var_name.to_string(),
                value,
            });
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        (fs, cwd, variables)
    }

    /// Extract SetVariable side effects into a map for easy assertion.
    fn extract_vars(result: &ExecResult) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for effect in &result.side_effects {
            if let BuiltinSideEffect::SetVariable { name, value } = effect {
                map.insert(name.clone(), value.clone());
            }
        }
        map
    }

    // ==================== no stdin ====================

    #[tokio::test]
    async fn read_no_stdin_returns_error() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ==================== basic read into REPLY ====================

    #[tokio::test]
    async fn read_into_reply() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hello world"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("REPLY").unwrap(), "hello world");
    }

    // ==================== read into named variable ====================

    #[tokio::test]
    async fn read_into_named_var() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["MY_VAR".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("test_value"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("MY_VAR").unwrap(), "test_value");
    }

    // ==================== read into multiple variables ====================

    #[tokio::test]
    async fn read_multiple_vars() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one two three four"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "two");
        // Last var gets remaining words
        assert_eq!(vars.get("C").unwrap(), "three four");
    }

    #[tokio::test]
    async fn read_more_vars_than_words() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "");
        assert_eq!(vars.get("C").unwrap(), "");
    }

    // ==================== -r flag (raw mode) ====================

    #[tokio::test]
    async fn read_raw_mode_preserves_backslash() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hello\\world"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("LINE").unwrap(), "hello\\world");
    }

    #[tokio::test]
    async fn read_without_raw_handles_line_continuation() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hello\\\nworld"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        // Without -r, backslash-newline is line continuation
        assert_eq!(vars.get("LINE").unwrap(), "helloworld");
    }

    // ==================== -n flag (read N chars) ====================

    #[tokio::test]
    async fn read_n_chars() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "3".to_string(), "CHUNK".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("abcdefgh"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("CHUNK").unwrap(), "abc");
    }

    #[tokio::test]
    async fn read_n_more_than_input() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "100".to_string(), "CHUNK".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hi"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("CHUNK").unwrap(), "hi");
    }

    // ==================== -d flag (delimiter) ====================

    #[tokio::test]
    async fn read_custom_delimiter() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-d".to_string(), ",".to_string(), "FIELD".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("first,second,third"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("FIELD").unwrap(), "first");
    }

    // ==================== -a flag (array mode) ====================

    #[tokio::test]
    async fn read_array_mode() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-a".to_string(), "ARR".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one two three"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.side_effects.len(), 1);
        match &result.side_effects[0] {
            BuiltinSideEffect::SetArray { name, elements } => {
                assert_eq!(name, "ARR");
                assert_eq!(elements, &["one", "two", "three"]);
            }
            _ => panic!("Expected SetArray side effect"),
        }
    }

    #[tokio::test]
    async fn read_array_mode_default_name() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-a".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("a b"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.side_effects.len(), 1);
        match &result.side_effects[0] {
            BuiltinSideEffect::SetArray { name, .. } => assert_eq!(name, "REPLY"),
            _ => panic!("Expected SetArray side effect"),
        }
    }

    // ==================== combined flags ====================

    #[tokio::test]
    async fn read_combined_r_flag() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        // -r combined in single arg
        let args = vec!["-r".to_string(), "V".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("path\\to\\file"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("V").unwrap(), "path\\to\\file");
    }

    // ==================== multiline input ====================

    #[tokio::test]
    async fn read_only_first_line() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("first\nsecond\nthird"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("LINE").unwrap(), "first");
    }

    // ==================== custom IFS ====================

    #[tokio::test]
    async fn read_custom_ifs() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ":".to_string());
        let args = vec!["A".to_string(), "B".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("foo:bar:baz"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "foo");
        assert_eq!(vars.get("B").unwrap(), "bar baz");
    }

    #[tokio::test]
    async fn read_custom_ifs_preserves_empty_fields() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ":".to_string());
        let args = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one::three:"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "");
        assert_eq!(vars.get("C").unwrap(), "three");
        assert_eq!(vars.get("D").unwrap(), "");
    }

    #[tokio::test]
    async fn read_empty_ifs_no_splitting() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), String::new());
        let args = vec!["LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("no splitting here"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("LINE").unwrap(), "no splitting here");
    }
}
