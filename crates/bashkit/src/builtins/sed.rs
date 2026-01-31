//! sed - Stream editor builtin
//!
//! Implements basic sed functionality.
//!
//! Usage:
//!   sed 's/pattern/replacement/' file
//!   sed 's/pattern/replacement/g' file    # global replacement
//!   sed -i 's/pattern/replacement/' file  # in-place edit
//!   echo "text" | sed 's/pattern/replacement/'
//!   sed -n '2p' file                      # print line 2
//!   sed '2d' file                         # delete line 2
//!   sed -e 's/a/b/' -e 's/c/d/' file     # multiple commands

use async_trait::async_trait;
use regex::Regex;

use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// sed command - stream editor
pub struct Sed;

#[derive(Debug)]
enum SedCommand {
    Substitute {
        pattern: Regex,
        replacement: String,
        global: bool,
        print_only: bool,
    },
    Delete,
    Print,
    Quit,
}

#[derive(Debug, Clone)]
enum Address {
    All,
    Line(usize),
    Range(usize, usize),
    Regex(Regex),
    Last,
}

impl Address {
    fn matches(&self, line_num: usize, total_lines: usize, line: &str) -> bool {
        match self {
            Address::All => true,
            Address::Line(n) => line_num == *n,
            Address::Range(start, end) => line_num >= *start && line_num <= *end,
            Address::Regex(re) => re.is_match(line),
            Address::Last => line_num == total_lines,
        }
    }
}

struct SedOptions {
    commands: Vec<(Option<Address>, SedCommand)>,
    files: Vec<String>,
    in_place: bool,
    quiet: bool,
}

impl SedOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = SedOptions {
            commands: Vec::new(),
            files: Vec::new(),
            in_place: false,
            quiet: false,
        };

        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "-n" {
                opts.quiet = true;
            } else if arg == "-i" {
                opts.in_place = true;
            } else if arg == "-e" {
                i += 1;
                if i < args.len() {
                    let (addr, cmd) = parse_sed_command(&args[i])?;
                    opts.commands.push((addr, cmd));
                }
            } else if arg.starts_with('-') {
                // Unknown option - ignore
            } else if opts.commands.is_empty() {
                // First non-option is the command
                let (addr, cmd) = parse_sed_command(arg)?;
                opts.commands.push((addr, cmd));
            } else {
                // Rest are files
                opts.files.push(arg.clone());
            }
            i += 1;
        }

        if opts.commands.is_empty() {
            return Err(Error::Execution("sed: no command given".to_string()));
        }

        Ok(opts)
    }
}

fn parse_address(s: &str) -> Result<(Option<Address>, &str)> {
    if s.is_empty() {
        return Ok((None, s));
    }

    let first_char = s.chars().next().unwrap();

    // Line number
    if first_char.is_ascii_digit() {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let num: usize = s[..end]
            .parse()
            .map_err(|_| Error::Execution("sed: invalid address".to_string()))?;
        let rest = &s[end..];

        // Check for range
        if let Some(rest) = rest.strip_prefix(',') {
            if let Some(after_dollar) = rest.strip_prefix('$') {
                return Ok((Some(Address::Range(num, usize::MAX)), after_dollar));
            }
            let end2 = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            if end2 > 0 {
                let num2: usize = rest[..end2]
                    .parse()
                    .map_err(|_| Error::Execution("sed: invalid address".to_string()))?;
                return Ok((Some(Address::Range(num, num2)), &rest[end2..]));
            }
            return Ok((Some(Address::Line(num)), rest));
        }

        return Ok((Some(Address::Line(num)), rest));
    }

    // Last line
    if let Some(after_dollar) = s.strip_prefix('$') {
        return Ok((Some(Address::Last), after_dollar));
    }

    // Regex address /pattern/
    if first_char == '/' {
        let end = s[1..]
            .find('/')
            .ok_or_else(|| Error::Execution("sed: unterminated address regex".to_string()))?;
        let pattern = &s[1..end + 1];
        let regex = Regex::new(pattern)
            .map_err(|e| Error::Execution(format!("sed: invalid regex: {}", e)))?;
        return Ok((Some(Address::Regex(regex)), &s[end + 2..]));
    }

    Ok((None, s))
}

fn parse_sed_command(s: &str) -> Result<(Option<Address>, SedCommand)> {
    let (address, rest) = parse_address(s)?;

    if rest.is_empty() {
        return Err(Error::Execution("sed: missing command".to_string()));
    }

    let first_char = rest.chars().next().unwrap();

    match first_char {
        's' => {
            // Substitution: s/pattern/replacement/flags
            if rest.len() < 4 {
                return Err(Error::Execution("sed: invalid substitution".to_string()));
            }
            let delim = rest.chars().nth(1).unwrap();

            // Find the parts between delimiters
            let rest = &rest[2..];
            let mut parts = Vec::new();
            let mut current = String::new();
            let mut escaped = false;

            for c in rest.chars() {
                if escaped {
                    current.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                    current.push(c);
                } else if c == delim {
                    parts.push(current);
                    current = String::new();
                } else {
                    current.push(c);
                }
            }
            parts.push(current);

            if parts.len() < 2 {
                return Err(Error::Execution("sed: invalid substitution".to_string()));
            }

            let pattern = &parts[0];
            let replacement = &parts[1];
            let flags = parts.get(2).map(|s| s.as_str()).unwrap_or("");

            // Convert POSIX sed regex to Rust regex syntax
            // \( \) -> ( ) for capture groups
            // \+ -> + for one-or-more
            // \? -> ? for zero-or-one
            let pattern = pattern
                .replace("\\(", "(")
                .replace("\\)", ")")
                .replace("\\+", "+")
                .replace("\\?", "?");

            let regex = Regex::new(&pattern)
                .map_err(|e| Error::Execution(format!("sed: invalid pattern: {}", e)))?;

            // Convert sed replacement syntax to regex replacement syntax
            // sed uses \1, \2, etc. and & for full match
            // regex crate uses $1, $2, etc. and $0 for full match
            let replacement = replacement
                .replace("\\&", "\x00") // Temporarily escape literal &
                .replace('&', "$0")
                .replace("\x00", "&");

            let replacement = Regex::new(r"\\(\d+)")
                .unwrap()
                .replace_all(&replacement, "$$$1")
                .to_string();

            Ok((
                address,
                SedCommand::Substitute {
                    pattern: regex,
                    replacement,
                    global: flags.contains('g'),
                    print_only: flags.contains('p'),
                },
            ))
        }
        'd' => Ok((address.or(Some(Address::All)), SedCommand::Delete)),
        'p' => Ok((address.or(Some(Address::All)), SedCommand::Print)),
        'q' => Ok((address, SedCommand::Quit)),
        _ => Err(Error::Execution(format!(
            "sed: unknown command: {}",
            first_char
        ))),
    }
}

#[async_trait]
impl Builtin for Sed {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let opts = SedOptions::parse(ctx.args)?;

        // Determine input
        let inputs: Vec<(Option<String>, String)> = if opts.files.is_empty() {
            vec![(None, ctx.stdin.unwrap_or("").to_string())]
        } else {
            let mut inputs = Vec::new();
            for file in &opts.files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content).into_owned();
                        inputs.push((Some(file.clone()), text));
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("sed: {}: {}", file, e), 1));
                    }
                }
            }
            inputs
        };

        let mut output = String::new();
        let mut modified_files: Vec<(String, String)> = Vec::new();

        for (filename, content) in inputs {
            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let mut file_output = String::new();
            let mut quit = false;

            for (idx, line) in lines.iter().enumerate() {
                if quit {
                    break;
                }

                let line_num = idx + 1;
                let mut current_line = line.to_string();
                let mut should_print = !opts.quiet;
                let mut deleted = false;
                let mut extra_print = false;

                for (addr, cmd) in &opts.commands {
                    let addr_matches = addr
                        .as_ref()
                        .map(|a| a.matches(line_num, total_lines, &current_line))
                        .unwrap_or(true);

                    if !addr_matches {
                        continue;
                    }

                    match cmd {
                        SedCommand::Substitute {
                            pattern,
                            replacement,
                            global,
                            print_only,
                        } => {
                            let new_line = if *global {
                                pattern.replace_all(&current_line, replacement.as_str())
                            } else {
                                pattern.replace(&current_line, replacement.as_str())
                            };

                            if new_line != current_line {
                                current_line = new_line.into_owned();
                                if *print_only {
                                    extra_print = true;
                                }
                            }
                        }
                        SedCommand::Delete => {
                            deleted = true;
                            should_print = false;
                        }
                        SedCommand::Print => {
                            extra_print = true;
                        }
                        SedCommand::Quit => {
                            quit = true;
                        }
                    }
                }

                if !deleted && should_print {
                    file_output.push_str(&current_line);
                    file_output.push('\n');
                }

                if extra_print {
                    file_output.push_str(&current_line);
                    file_output.push('\n');
                }
            }

            if opts.in_place {
                if let Some(fname) = filename {
                    modified_files.push((fname, file_output));
                }
            } else {
                output.push_str(&file_output);
            }
        }

        // Write back in-place modifications
        for (filename, content) in modified_files {
            let path = if filename.starts_with('/') {
                std::path::PathBuf::from(&filename)
            } else {
                ctx.cwd.join(&filename)
            };

            if let Err(e) = ctx.fs.write_file(&path, content.as_bytes()).await {
                return Ok(ExecResult::err(format!("sed: {}: {}", filename, e), 1));
            }
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_sed(args: &[&str], stdin: Option<&str>) -> Result<ExecResult> {
        let sed = Sed;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin,
        };

        sed.execute(ctx).await
    }

    #[tokio::test]
    async fn test_sed_substitute() {
        let result = run_sed(&["s/hello/goodbye/"], Some("hello world\nhello again"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "goodbye world\ngoodbye again\n");
    }

    #[tokio::test]
    async fn test_sed_substitute_global() {
        let result = run_sed(&["s/o/0/g"], Some("hello world")).await.unwrap();
        assert_eq!(result.stdout, "hell0 w0rld\n");
    }

    #[tokio::test]
    async fn test_sed_substitute_first_only() {
        let result = run_sed(&["s/o/0/"], Some("hello world")).await.unwrap();
        assert_eq!(result.stdout, "hell0 world\n");
    }

    #[tokio::test]
    async fn test_sed_delete_line() {
        let result = run_sed(&["2d"], Some("line1\nline2\nline3")).await.unwrap();
        assert_eq!(result.stdout, "line1\nline3\n");
    }

    #[tokio::test]
    async fn test_sed_print_line() {
        let result = run_sed(&["-n", "2p"], Some("line1\nline2\nline3"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "line2\n");
    }

    #[tokio::test]
    async fn test_sed_regex_groups() {
        let result = run_sed(&["s/\\(hello\\) \\(world\\)/\\2 \\1/"], Some("hello world"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "world hello\n");
    }

    #[tokio::test]
    async fn test_sed_ampersand() {
        let result = run_sed(&["s/world/[&]/"], Some("hello world"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello [world]\n");
    }

    #[tokio::test]
    async fn test_sed_address_range() {
        let result = run_sed(&["2,3d"], Some("line1\nline2\nline3\nline4"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "line1\nline4\n");
    }

    #[tokio::test]
    async fn test_sed_last_line() {
        let result = run_sed(&["$d"], Some("line1\nline2\nline3")).await.unwrap();
        assert_eq!(result.stdout, "line1\nline2\n");
    }
}
