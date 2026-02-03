//! sed - Stream editor builtin
//!
//! Implements basic sed functionality.
//!
//! Usage:
//!   sed 's/pattern/replacement/' file
//!   sed 's/pattern/replacement/g' file    # global replacement
//!   sed 's/pattern/replacement/2' file    # nth occurrence
//!   sed -E 's/pattern+/replacement/' file # extended regex
//!   sed -i 's/pattern/replacement/' file  # in-place edit
//!   echo "text" | sed 's/pattern/replacement/'
//!   sed -n '2p' file                      # print line 2
//!   sed '2d' file                         # delete line 2
//!   sed '/bar/!d' file                    # delete lines not matching bar
//!   sed -e 's/a/b/' -e 's/c/d/' file     # multiple commands

// sed command parser uses chars().next().unwrap() after validating.
// This is safe because we check for non-empty strings before accessing.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use regex::{Regex, RegexBuilder};

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
        nth: Option<usize>, // Replace nth occurrence (1-indexed)
        print_only: bool,
    },
    Delete,
    Print,
    Quit,
    Append(String),
    Insert(String),
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
    commands: Vec<(Option<Address>, bool, SedCommand)>, // (address, negate, command)
    files: Vec<String>,
    in_place: bool,
    quiet: bool,
    extended_regex: bool,
}

impl SedOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = SedOptions {
            commands: Vec::new(),
            files: Vec::new(),
            in_place: false,
            quiet: false,
            extended_regex: false,
        };

        // First pass: check for -E flag
        for arg in args {
            if arg == "-E" || arg == "-r" {
                opts.extended_regex = true;
            }
        }

        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "-n" {
                opts.quiet = true;
            } else if arg == "-i" {
                opts.in_place = true;
            } else if arg == "-E" || arg == "-r" {
                // Already handled
            } else if arg == "-e" {
                i += 1;
                if i < args.len() {
                    let (addr, negate, cmd) = parse_sed_command(&args[i], opts.extended_regex)?;
                    opts.commands.push((addr, negate, cmd));
                }
            } else if arg.starts_with('-') {
                // Unknown option - ignore
            } else if opts.commands.is_empty() {
                // First non-option is the command (may contain multiple commands separated by ;)
                for cmd_str in split_sed_commands(arg) {
                    let trimmed = cmd_str.trim();
                    if !trimmed.is_empty() {
                        let (addr, negate, cmd) = parse_sed_command(trimmed, opts.extended_regex)?;
                        opts.commands.push((addr, negate, cmd));
                    }
                }
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

/// Split a sed command string into individual commands separated by semicolons.
/// This is careful to not split inside s/pattern/replacement/ structures.
fn split_sed_commands(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut in_subst = false;
    let mut delim_count = 0;
    let mut delim: Option<char> = None;
    let mut escaped = false;
    let chars: Vec<char> = s.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            continue;
        }

        if !in_subst && c == 's' && i + 1 < chars.len() {
            // Start of substitution command
            in_subst = true;
            delim = Some(chars[i + 1]);
            delim_count = 0;
        } else if in_subst {
            if Some(c) == delim {
                delim_count += 1;
                if delim_count >= 3 {
                    // After third delimiter, we might have flags then end
                    in_subst = false;
                }
            }
        } else if c == ';' {
            result.push(&s[start..i]);
            start = i + 1;
        }
    }

    if start < s.len() {
        result.push(&s[start..]);
    }

    result
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

fn parse_sed_command(s: &str, extended_regex: bool) -> Result<(Option<Address>, bool, SedCommand)> {
    let (address, rest) = parse_address(s)?;

    if rest.is_empty() {
        return Err(Error::Execution("sed: missing command".to_string()));
    }

    // Check for address negation (!)
    let (negate, rest) = if let Some(r) = rest.strip_prefix('!') {
        (true, r)
    } else {
        (false, rest)
    };

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
            // In BRE mode: \( \) -> ( ) for capture groups, \+ -> +, \? -> ?
            // In ERE mode: ( ) are already groups, + and ? work directly
            let pattern = if extended_regex {
                // ERE mode: no conversion needed for groups/quantifiers
                pattern.clone()
            } else {
                // BRE mode: convert escaped metacharacters
                pattern
                    .replace("\\(", "(")
                    .replace("\\)", ")")
                    .replace("\\+", "+")
                    .replace("\\?", "?")
            };
            // Build regex with optional case-insensitive flag
            let case_insensitive = flags.contains('i');
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(case_insensitive)
                .build()
                .map_err(|e| Error::Execution(format!("sed: invalid pattern: {}", e)))?;

            // Convert sed replacement syntax to regex replacement syntax
            // sed uses \1, \2, etc. and & for full match
            // regex crate uses $1, $2, etc. and $0 for full match
            let replacement = replacement
                .replace("\\&", "\x00") // Temporarily escape literal &
                .replace('&', "$0")
                .replace("\x00", "&");

            // Use ${N} format instead of $N to avoid ambiguity with following chars
            let replacement = Regex::new(r"\\(\d+)")
                .unwrap()
                .replace_all(&replacement, r"$${$1}")
                .to_string();

            // Parse nth occurrence from flags (e.g., "2" in s/a/b/2)
            let nth = flags
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<usize>()
                .ok()
                .filter(|&n| n > 0);

            Ok((
                address,
                negate,
                SedCommand::Substitute {
                    pattern: regex,
                    replacement,
                    global: flags.contains('g'),
                    nth,
                    print_only: flags.contains('p'),
                },
            ))
        }
        'd' => Ok((address.or(Some(Address::All)), negate, SedCommand::Delete)),
        'p' => Ok((address.or(Some(Address::All)), negate, SedCommand::Print)),
        'q' => Ok((address, negate, SedCommand::Quit)),
        'a' => {
            // Append command: a\text or a text (after backslash)
            let text = if rest.len() > 1 && rest.chars().nth(1) == Some('\\') {
                rest[2..].to_string()
            } else {
                rest[1..].to_string()
            };
            Ok((address, negate, SedCommand::Append(text)))
        }
        'i' => {
            // Insert command: i\text or i text (after backslash)
            let text = if rest.len() > 1 && rest.chars().nth(1) == Some('\\') {
                rest[2..].to_string()
            } else {
                rest[1..].to_string()
            };
            Ok((address, negate, SedCommand::Insert(text)))
        }
        _ => Err(Error::Execution(format!(
            "sed: unknown command: {}",
            first_char
        ))),
    }
}

/// Replace the nth occurrence of a pattern in a string
fn replace_nth<'a>(
    pattern: &Regex,
    text: &'a str,
    replacement: &str,
    n: usize,
) -> std::borrow::Cow<'a, str> {
    let mut count = 0;

    for mat in pattern.find_iter(text) {
        count += 1;
        if count == n {
            // Found the nth match - do the replacement
            let mut result = String::new();
            result.push_str(&text[..mat.start()]);
            // Apply replacement with capture groups
            let replaced = pattern.replace(mat.as_str(), replacement);
            result.push_str(&replaced);
            // Add the rest of the string
            result.push_str(&text[mat.end()..]);
            return std::borrow::Cow::Owned(result);
        }
    }

    // nth occurrence not found, return original
    std::borrow::Cow::Borrowed(text)
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
                let mut insert_text: Option<String> = None;
                let mut append_text: Option<String> = None;

                for (addr, negate, cmd) in &opts.commands {
                    let addr_matches = addr
                        .as_ref()
                        .map(|a| a.matches(line_num, total_lines, &current_line))
                        .unwrap_or(true);

                    // Apply negation if needed
                    let should_apply = if *negate { !addr_matches } else { addr_matches };

                    if !should_apply {
                        continue;
                    }

                    match cmd {
                        SedCommand::Substitute {
                            pattern,
                            replacement,
                            global,
                            nth,
                            print_only,
                        } => {
                            let new_line = if *global {
                                pattern.replace_all(&current_line, replacement.as_str())
                            } else if let Some(n) = nth {
                                // Replace nth occurrence
                                replace_nth(pattern, &current_line, replacement, *n)
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
                        SedCommand::Append(text) => {
                            append_text = Some(text.clone());
                        }
                        SedCommand::Insert(text) => {
                            insert_text = Some(text.clone());
                        }
                    }
                }

                // Insert text comes before the line
                if let Some(text) = insert_text {
                    file_output.push_str(&text);
                    file_output.push('\n');
                }

                if !deleted && should_print {
                    file_output.push_str(&current_line);
                    file_output.push('\n');
                }

                if extra_print {
                    file_output.push_str(&current_line);
                    file_output.push('\n');
                }

                // Append text comes after the line
                if let Some(text) = append_text {
                    file_output.push_str(&text);
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
#[allow(clippy::unwrap_used)]
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
            #[cfg(feature = "http_client")]
            http_client: None,
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
    async fn test_sed_backref_single() {
        // Test single backreference: capture "hel", replace entire match with captured + "p"
        let result = run_sed(&["s/\\(hel\\)lo/\\1p/"], Some("hello"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "help\n");
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

    #[tokio::test]
    async fn test_sed_case_insensitive() {
        let result = run_sed(&["s/hello/hi/i"], Some("Hello World"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hi World\n");
    }

    #[tokio::test]
    async fn test_sed_multiple_commands() {
        let result = run_sed(&["s/hello/hi/; s/world/there/"], Some("hello world"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hi there\n");
    }

    #[tokio::test]
    async fn test_sed_append() {
        let result = run_sed(&["/one/a\\inserted"], Some("one\ntwo"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "one\ninserted\ntwo\n");
    }

    #[tokio::test]
    async fn test_sed_insert() {
        let result = run_sed(&["/two/i\\inserted"], Some("one\ntwo"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "one\ninserted\ntwo\n");
    }
}
