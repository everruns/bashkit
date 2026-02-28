//! Cut and tr builtins - extract fields and translate characters

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The cut builtin - remove sections from each line.
///
/// Usage: cut -d DELIM -f FIELDS [FILE...]
///        cut -b BYTES [FILE...]
///        cut -c CHARS [FILE...]
///
/// Options:
///   -d DELIM            Use DELIM instead of TAB for field delimiter
///   -f FIELDS           Select only these fields (1-indexed)
///   -b BYTES            Select only these bytes (1-indexed, same as -c for ASCII)
///   -c CHARS            Select only these characters (1-indexed)
///   -s                  Only print lines containing delimiter (with -f)
///   --complement        Complement the selection
///   --output-delimiter  Use STRING as output delimiter
pub struct Cut;

#[derive(PartialEq)]
enum CutMode {
    Fields,
    Chars,
}

#[async_trait]
impl Builtin for Cut {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut delimiter = '\t';
        let mut spec = String::new();
        let mut mode = CutMode::Fields;
        let mut complement = false;
        let mut only_delimited = false;
        let mut zero_terminated = false;
        let mut output_delimiter: Option<String> = None;
        let mut files = Vec::new();

        // Parse arguments
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-d" {
                i += 1;
                if i < ctx.args.len() {
                    delimiter = ctx.args[i].chars().next().unwrap_or('\t');
                }
            } else if let Some(d) = arg.strip_prefix("-d") {
                delimiter = d.chars().next().unwrap_or('\t');
            } else if arg == "-f" {
                i += 1;
                if i < ctx.args.len() {
                    spec = ctx.args[i].clone();
                    mode = CutMode::Fields;
                }
            } else if let Some(f) = arg.strip_prefix("-f") {
                spec = f.to_string();
                mode = CutMode::Fields;
            } else if arg == "-c" || arg == "-b" {
                i += 1;
                if i < ctx.args.len() {
                    spec = ctx.args[i].clone();
                    mode = CutMode::Chars;
                }
            } else if let Some(c) = arg.strip_prefix("-c") {
                spec = c.to_string();
                mode = CutMode::Chars;
            } else if let Some(b) = arg.strip_prefix("-b") {
                spec = b.to_string();
                mode = CutMode::Chars;
            } else if arg == "-s" {
                only_delimited = true;
            } else if arg == "-z" {
                zero_terminated = true;
            } else if arg == "--complement" {
                complement = true;
            } else if let Some(od) = arg.strip_prefix("--output-delimiter=") {
                output_delimiter = Some(od.to_string());
            } else if arg == "--output-delimiter" {
                i += 1;
                if i < ctx.args.len() {
                    output_delimiter = Some(ctx.args[i].clone());
                }
            } else if !arg.starts_with('-') {
                files.push(arg.clone());
            }
            i += 1;
        }

        if spec.is_empty() {
            return Ok(ExecResult::err(
                "cut: you must specify a list of fields\n".to_string(),
                1,
            ));
        }

        // Parse position specification (supports open-ended ranges like "3-" and "-3")
        let positions = parse_position_spec(&spec);
        let out_delim = output_delimiter.unwrap_or_else(|| delimiter.to_string());

        let process_line = |line: &str| -> Option<String> {
            match mode {
                CutMode::Chars => {
                    let chars: Vec<char> = line.chars().collect();
                    let total = chars.len();
                    let resolved = resolve_positions(&positions, total);
                    let selected: Vec<char> = if complement {
                        chars
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| !resolved.contains(&(i + 1)))
                            .map(|(_, c)| *c)
                            .collect()
                    } else {
                        resolved
                            .iter()
                            .filter_map(|&p| chars.get(p - 1).copied())
                            .collect()
                    };
                    Some(selected.into_iter().collect())
                }
                CutMode::Fields => {
                    // -s: skip lines without delimiter
                    if only_delimited && !line.contains(delimiter) {
                        return None;
                    }
                    let parts: Vec<&str> = line.split(delimiter).collect();
                    let total = parts.len();
                    let resolved = resolve_positions(&positions, total);
                    let selected: Vec<&str> = if complement {
                        parts
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| !resolved.contains(&(i + 1)))
                            .map(|(_, s)| *s)
                            .collect()
                    } else {
                        resolved
                            .iter()
                            .filter_map(|&f| parts.get(f - 1).copied())
                            .collect()
                    };
                    Some(selected.join(&out_delim))
                }
            }
        };

        let mut output = String::new();
        let line_sep = if zero_terminated { '\0' } else { '\n' };
        let out_sep = if zero_terminated { "\0" } else { "\n" };

        let process_input = |text: &str, output: &mut String| {
            for line in text.split(line_sep) {
                if line.is_empty() {
                    continue;
                }
                if let Some(result) = process_line(line) {
                    output.push_str(&result);
                    output.push_str(out_sep);
                }
            }
        };

        if files.is_empty() || files.iter().all(|f| f.as_str() == "-") {
            if let Some(stdin) = ctx.stdin {
                process_input(stdin, &mut output);
            }
        } else {
            for file in &files {
                if file.as_str() == "-" {
                    if let Some(stdin) = ctx.stdin {
                        process_input(stdin, &mut output);
                    }
                    continue;
                }

                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        process_input(&text, &mut output);
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("cut: {}: {}\n", file, e), 1));
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Position in a cut specification — can be open-ended
#[derive(Debug, Clone)]
enum Position {
    Single(usize),
    Range(usize, usize),
    FromStart(usize), // -N (1 to N)
    ToEnd(usize),     // N- (N to end)
}

/// Parse a position specification like "1", "1,3", "1-3", "3-", "-3"
fn parse_position_spec(spec: &str) -> Vec<Position> {
    let mut positions = Vec::new();

    for part in spec.split(',') {
        if let Some((start, end)) = part.split_once('-') {
            if start.is_empty() {
                // -N
                if let Ok(n) = end.parse::<usize>() {
                    positions.push(Position::FromStart(n));
                }
            } else if end.is_empty() {
                // N-
                if let Ok(n) = start.parse::<usize>() {
                    positions.push(Position::ToEnd(n));
                }
            } else {
                // N-M
                let s: usize = start.parse().unwrap_or(1);
                let e: usize = end.parse().unwrap_or(s);
                positions.push(Position::Range(s, e));
            }
        } else if let Ok(f) = part.parse::<usize>() {
            if f > 0 {
                positions.push(Position::Single(f));
            }
        }
    }

    positions
}

/// Resolve position specifications into concrete 1-indexed positions
fn resolve_positions(positions: &[Position], total: usize) -> Vec<usize> {
    let mut result = Vec::new();
    for pos in positions {
        match pos {
            Position::Single(n) => {
                if *n > 0 && *n <= total {
                    result.push(*n);
                }
            }
            Position::Range(s, e) => {
                let start = (*s).max(1);
                let end = (*e).min(total);
                for i in start..=end {
                    result.push(i);
                }
            }
            Position::FromStart(n) => {
                for i in 1..=(*n).min(total) {
                    result.push(i);
                }
            }
            Position::ToEnd(n) => {
                let start = (*n).max(1);
                for i in start..=total {
                    result.push(i);
                }
            }
        }
    }
    result.sort();
    result.dedup();
    result
}

/// The tr builtin - translate or delete characters.
///
/// Usage: tr [-d] [-s] [-c/-C] SET1 [SET2]
///
/// Options:
///   -d     Delete characters in SET1
///   -s     Squeeze repeated output characters in SET2 (or SET1 if no SET2)
///   -c/-C  Complement SET1 (use all chars NOT in SET1)
///
/// SET1 and SET2 can contain character ranges like a-z, A-Z, 0-9
/// and POSIX classes like [:lower:], [:upper:], [:digit:], [:alpha:],
/// [:alnum:], [:space:], [:blank:], [:punct:], [:xdigit:], [:print:], [:graph:]
pub struct Tr;

#[async_trait]
impl Builtin for Tr {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut delete = false;
        let mut squeeze = false;
        let mut complement = false;

        // Parse flags (can be combined like -ds, -cd)
        let mut non_flag_args: Vec<&String> = Vec::new();
        for arg in ctx.args.iter() {
            if arg.starts_with('-')
                && arg.len() > 1
                && arg.chars().skip(1).all(|ch| "dscC".contains(ch))
            {
                for ch in arg.chars().skip(1) {
                    match ch {
                        'd' => delete = true,
                        's' => squeeze = true,
                        'c' | 'C' => complement = true,
                        _ => {}
                    }
                }
            } else {
                non_flag_args.push(arg);
            }
        }

        if non_flag_args.is_empty() {
            return Ok(ExecResult::err("tr: missing operand\n".to_string(), 1));
        }

        let mut set1 = expand_char_set(non_flag_args[0]);
        if complement {
            // Complement: use all ASCII chars NOT in set1
            let original = set1.clone();
            set1 = (0u8..=127)
                .map(|b| b as char)
                .filter(|c| !original.contains(c))
                .collect();
        }

        let stdin = ctx.stdin.unwrap_or("");

        let result = if delete && squeeze {
            // -ds: delete SET1 chars, then squeeze SET2 chars
            let set2 = if non_flag_args.len() >= 2 {
                expand_char_set(non_flag_args[1])
            } else {
                set1.clone()
            };
            let after_delete: String = stdin.chars().filter(|c| !set1.contains(c)).collect();
            squeeze_chars(&after_delete, &set2)
        } else if delete {
            stdin
                .chars()
                .filter(|c| !set1.contains(c))
                .collect::<String>()
        } else if squeeze && non_flag_args.len() < 2 {
            // -s with only SET1: squeeze characters in SET1
            squeeze_chars(stdin, &set1)
        } else {
            if non_flag_args.len() < 2 {
                return Ok(ExecResult::err(
                    "tr: missing operand after SET1\n".to_string(),
                    1,
                ));
            }

            let set2 = expand_char_set(non_flag_args[1]);

            let translated: String = stdin
                .chars()
                .map(|c| {
                    if let Some(pos) = set1.iter().position(|&x| x == c) {
                        *set2.get(pos).or(set2.last()).unwrap_or(&c)
                    } else {
                        c
                    }
                })
                .collect();

            if squeeze {
                squeeze_chars(&translated, &set2)
            } else {
                translated
            }
        };

        Ok(ExecResult::ok(result))
    }
}

/// Squeeze repeated consecutive characters that are in the given set
fn squeeze_chars(s: &str, set: &[char]) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_char: Option<char> = None;

    for c in s.chars() {
        if set.contains(&c) && last_char == Some(c) {
            continue; // skip repeated char in squeeze set
        }
        result.push(c);
        last_char = Some(c);
    }
    result
}

/// Expand a character set specification like "a-z" into a list of characters.
/// Supports POSIX character classes: [:lower:], [:upper:], [:digit:], [:alpha:], [:alnum:], [:space:]
fn expand_char_set(spec: &str) -> Vec<char> {
    let mut chars = Vec::new();
    let mut i = 0;
    let bytes = spec.as_bytes();

    while i < bytes.len() {
        // Check for POSIX character class [:class:]
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            if let Some(end) = spec[i + 2..].find(":]") {
                let class_name = &spec[i + 2..i + 2 + end];
                match class_name {
                    "lower" => chars.extend('a'..='z'),
                    "upper" => chars.extend('A'..='Z'),
                    "digit" => chars.extend('0'..='9'),
                    "alpha" => {
                        chars.extend('a'..='z');
                        chars.extend('A'..='Z');
                    }
                    "alnum" => {
                        chars.extend('a'..='z');
                        chars.extend('A'..='Z');
                        chars.extend('0'..='9');
                    }
                    "space" => chars.extend([' ', '\t', '\n', '\r', '\x0b', '\x0c']),
                    "blank" => chars.extend([' ', '\t']),
                    "punct" => {
                        // ASCII punctuation: !"#$%&'()*+,-./:;<=>?@[\]^_`{|}~
                        for code in 0x21u8..=0x7e {
                            let c = code as char;
                            if !c.is_ascii_alphanumeric() {
                                chars.push(c);
                            }
                        }
                    }
                    "xdigit" => {
                        chars.extend('0'..='9');
                        chars.extend('A'..='F');
                        chars.extend('a'..='f');
                    }
                    "print" => {
                        // Printable characters: space + graph
                        for code in 0x20u8..=0x7e {
                            chars.push(code as char);
                        }
                    }
                    "graph" => {
                        // Visible characters (no space)
                        for code in 0x21u8..=0x7e {
                            chars.push(code as char);
                        }
                    }
                    "cntrl" => {
                        for code in 0u8..=0x1f {
                            chars.push(code as char);
                        }
                        chars.push(0x7f as char);
                    }
                    _ => {
                        // Unknown class, treat literally
                        chars.push('[');
                        i += 1;
                        continue;
                    }
                }
                i += 2 + end + 2; // skip past [: + class + :]
                continue;
            }
        }

        let c = bytes[i] as char;
        // Check for range like a-z
        if i + 2 < bytes.len() && bytes[i + 1] == b'-' {
            let end = bytes[i + 2] as char;
            let start = c as u32;
            let end = end as u32;
            for code in start..=end {
                if let Some(ch) = char::from_u32(code) {
                    chars.push(ch);
                }
            }
            i += 3;
        } else if i + 1 == bytes.len() - 1 && bytes[i + 1] == b'-' {
            // Trailing dash
            chars.push(c);
            chars.push('-');
            i += 2;
        } else {
            // Handle escape sequences
            if c == '\\' && i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'n' => {
                        chars.push('\n');
                        i += 2;
                        continue;
                    }
                    b't' => {
                        chars.push('\t');
                        i += 2;
                        continue;
                    }
                    b'0' => {
                        chars.push('\0');
                        i += 2;
                        continue;
                    }
                    b'\\' => {
                        chars.push('\\');
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            chars.push(c);
            i += 1;
        }
    }

    chars
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_cut(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Cut.execute(ctx).await.unwrap()
    }

    async fn run_tr(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Tr.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_cut_single_field() {
        let result = run_cut(&["-d", ",", "-f", "2"], Some("a,b,c\n1,2,3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "b\n2\n");
    }

    #[tokio::test]
    async fn test_cut_multiple_fields() {
        let result = run_cut(&["-d", ",", "-f", "1,3"], Some("a,b,c\n1,2,3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,c\n1,3\n");
    }

    #[tokio::test]
    async fn test_cut_field_range() {
        let result = run_cut(&["-d", ",", "-f", "1-2"], Some("a,b,c,d\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,b\n");
    }

    #[tokio::test]
    async fn test_tr_lowercase_to_uppercase() {
        let result = run_tr(&["a-z", "A-Z"], Some("hello world")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "HELLO WORLD");
    }

    #[tokio::test]
    async fn test_tr_delete() {
        let result = run_tr(&["-d", "aeiou"], Some("hello world")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hll wrld");
    }

    #[tokio::test]
    async fn test_tr_single_char() {
        let result = run_tr(&[":", "-"], Some("a:b:c")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a-b-c");
    }

    #[test]
    fn test_expand_char_set() {
        assert_eq!(expand_char_set("abc"), vec!['a', 'b', 'c']);
        assert_eq!(expand_char_set("a-c"), vec!['a', 'b', 'c']);
        assert_eq!(expand_char_set("0-2"), vec!['0', '1', '2']);
    }

    #[test]
    fn test_expand_char_class_lower() {
        let lower = expand_char_set("[:lower:]");
        assert_eq!(lower.len(), 26);
        assert_eq!(lower[0], 'a');
        assert_eq!(lower[25], 'z');
    }

    #[test]
    fn test_expand_char_class_upper() {
        let upper = expand_char_set("[:upper:]");
        assert_eq!(upper.len(), 26);
        assert_eq!(upper[0], 'A');
        assert_eq!(upper[25], 'Z');
    }

    #[tokio::test]
    async fn test_tr_char_class_lower_to_upper() {
        let result = run_tr(&["[:lower:]", "[:upper:]"], Some("hello world\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "HELLO WORLD\n");
    }

    #[test]
    fn test_parse_position_spec() {
        // Resolved against 10 total positions
        let resolve = |spec: &str| resolve_positions(&parse_position_spec(spec), 10);
        assert_eq!(resolve("1"), vec![1]);
        assert_eq!(resolve("1,3"), vec![1, 3]);
        assert_eq!(resolve("1-3"), vec![1, 2, 3]);
        assert_eq!(resolve("1,3-5"), vec![1, 3, 4, 5]);
        assert_eq!(resolve("3-"), vec![3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(resolve("-3"), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_cut_char_mode() {
        let result = run_cut(&["-c", "1-5"], Some("hello world\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_cut_complement() {
        let result = run_cut(&["-d", ",", "--complement", "-f", "2"], Some("a,b,c,d\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,c,d\n");
    }

    #[tokio::test]
    async fn test_cut_only_delimited() {
        let result = run_cut(
            &["-d", ",", "-f", "1", "-s"],
            Some("a,b,c\nno delim\nx,y\n"),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nx\n");
    }

    #[tokio::test]
    async fn test_tr_squeeze() {
        let result = run_tr(&["-s", "eol "], Some("heeelllo   wooorld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "helo world\n");
    }

    #[tokio::test]
    async fn test_tr_complement_delete() {
        let result = run_tr(&["-cd", "0-9\n"], Some("hello123\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "123\n");
    }

    #[tokio::test]
    async fn test_tr_complement_uppercase_c() {
        // -C is POSIX alias for -c (complement)
        let result = run_tr(&["-Cd", "0-9\n"], Some("hello123\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "123\n");
    }

    #[tokio::test]
    async fn test_tr_combined_flags_ds() {
        // -ds: delete SET1 chars, then squeeze SET2 chars
        let result = run_tr(&["-ds", "aeiou", " "], Some("the  quick  fox\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "th qck fx\n");
    }

    #[test]
    fn test_expand_char_class_punct() {
        let punct = expand_char_set("[:punct:]");
        assert!(punct.contains(&'!'));
        assert!(punct.contains(&'.'));
        assert!(punct.contains(&','));
        assert!(punct.contains(&'@'));
        assert!(punct.contains(&'#'));
        assert!(!punct.contains(&'a'));
        assert!(!punct.contains(&'0'));
        assert!(!punct.contains(&' '));
    }

    #[test]
    fn test_expand_char_class_xdigit() {
        let xdigit = expand_char_set("[:xdigit:]");
        assert_eq!(xdigit.len(), 22); // 0-9 + A-F + a-f
        assert!(xdigit.contains(&'0'));
        assert!(xdigit.contains(&'9'));
        assert!(xdigit.contains(&'A'));
        assert!(xdigit.contains(&'F'));
        assert!(xdigit.contains(&'a'));
        assert!(xdigit.contains(&'f'));
        assert!(!xdigit.contains(&'G'));
        assert!(!xdigit.contains(&'g'));
    }

    #[test]
    fn test_expand_char_class_digit() {
        let digit = expand_char_set("[:digit:]");
        assert_eq!(digit.len(), 10);
        assert_eq!(digit[0], '0');
        assert_eq!(digit[9], '9');
    }

    #[test]
    fn test_expand_char_class_alpha() {
        let alpha = expand_char_set("[:alpha:]");
        assert_eq!(alpha.len(), 52);
        assert!(alpha.contains(&'a'));
        assert!(alpha.contains(&'z'));
        assert!(alpha.contains(&'A'));
        assert!(alpha.contains(&'Z'));
    }

    #[test]
    fn test_expand_char_class_alnum() {
        let alnum = expand_char_set("[:alnum:]");
        assert_eq!(alnum.len(), 62);
        assert!(alnum.contains(&'a'));
        assert!(alnum.contains(&'0'));
        assert!(alnum.contains(&'Z'));
    }

    #[test]
    fn test_expand_char_class_space() {
        let space = expand_char_set("[:space:]");
        assert!(space.contains(&' '));
        assert!(space.contains(&'\t'));
        assert!(space.contains(&'\n'));
        assert!(space.contains(&'\r'));
    }

    #[test]
    fn test_expand_char_class_blank() {
        let blank = expand_char_set("[:blank:]");
        assert_eq!(blank.len(), 2);
        assert!(blank.contains(&' '));
        assert!(blank.contains(&'\t'));
    }

    #[test]
    fn test_expand_char_class_cntrl() {
        let cntrl = expand_char_set("[:cntrl:]");
        assert!(cntrl.contains(&'\0'));
        assert!(cntrl.contains(&'\x1f'));
        assert!(cntrl.contains(&'\x7f'));
        assert!(!cntrl.contains(&' '));
    }

    #[tokio::test]
    async fn test_tr_delete_punct() {
        let result = run_tr(&["-d", "[:punct:]"], Some("hello, world!\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_tr_squeeze_spaces() {
        let result = run_tr(&["-s", "[:space:]"], Some("hello   world\n\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_tr_translate_with_squeeze() {
        let result = run_tr(&["-s", "a-z", "A-Z"], Some("aabbcc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "ABC\n");
    }

    #[tokio::test]
    async fn test_cut_byte_mode() {
        // -b is alias for -c
        let result = run_cut(&["-b", "1-5"], Some("hello world\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_cut_byte_mode_inline() {
        let result = run_cut(&["-b1,3,5"], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hlo\n");
    }

    #[tokio::test]
    async fn test_tr_complement_squeeze() {
        // -cs: complement SET1, then squeeze result chars in SET2
        let result = run_tr(&["-cs", "[:alpha:]", "\n"], Some("hello 123 world\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\nworld\n");
    }
}
