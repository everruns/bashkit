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

/// Convert a BRE (Basic Regular Expression) pattern to ERE for the regex crate.
/// In BRE: ( ) { } are literal; \( \) \{ \} \+ \? \| are metacharacters.
fn bre_to_ere(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                '(' | ')' | '{' | '}' | '+' | '?' | '|' => {
                    result.push(chars[i + 1]);
                    i += 2;
                }
                _ => {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else if chars[i] == '(' || chars[i] == ')' || chars[i] == '{' || chars[i] == '}' {
            result.push('\\');
            result.push(chars[i]);
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

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
    QuitNoprint, // Q - quit without printing
    Append(String),
    Insert(String),
    Change(String),                                  // c\text - replace line
    HoldCopy,                                        // h - copy pattern to hold
    HoldAppend,                                      // H - append pattern to hold
    GetCopy,                                         // g - copy hold to pattern
    GetAppend,                                       // G - append hold to pattern
    Exchange,                                        // x - swap pattern and hold
    Group(Vec<(Option<Address>, bool, SedCommand)>), // { cmd1; cmd2; ... }
    Label(String),                                   // :label
    Branch(Option<String>),                          // b [label] - unconditional branch
    BranchIfSub(Option<String>),                     // t [label] - branch if substitution succeeded
}

#[derive(Debug, Clone)]
enum Address {
    All,
    Line(usize),
    Range(usize, usize),
    Regex(Regex),
    Last,
    RegexRange(Regex, Regex),     // /start/,/end/ - regex range
    LineRegexRange(usize, Regex), // N,/end/
    Step(usize, usize),           // first~step - every step-th line starting at first
    ZeroRegex(Regex),             // 0,/pattern/ - from line 0 to first match
}

impl Address {
    fn matches_simple(&self, line_num: usize, total_lines: usize, line: &str) -> bool {
        match self {
            Address::All => true,
            Address::Line(n) => line_num == *n,
            Address::Range(start, end) => line_num >= *start && line_num <= *end,
            Address::Regex(re) => re.is_match(line),
            Address::Last => line_num == total_lines,
            Address::Step(first, step) => {
                if *step == 0 {
                    line_num == *first
                } else if *first == 0 {
                    line_num.is_multiple_of(*step)
                } else {
                    line_num >= *first && (line_num - *first).is_multiple_of(*step)
                }
            }
            // Ranges with state handled separately
            Address::RegexRange(_, _) | Address::LineRegexRange(_, _) | Address::ZeroRegex(_) => {
                false
            }
        }
    }

    fn matches_with_state(
        &self,
        line_num: usize,
        total_lines: usize,
        line: &str,
        in_range: &mut bool,
    ) -> bool {
        match self {
            Address::RegexRange(start_re, end_re) => {
                if *in_range {
                    if end_re.is_match(line) {
                        *in_range = false;
                    }
                    true
                } else if start_re.is_match(line) {
                    *in_range = true;
                    true
                } else {
                    false
                }
            }
            Address::LineRegexRange(start_line, end_re) => {
                if *in_range {
                    if end_re.is_match(line) {
                        *in_range = false;
                    }
                    true
                } else if line_num >= *start_line {
                    *in_range = true;
                    true
                } else {
                    false
                }
            }
            Address::ZeroRegex(end_re) => {
                // 0,/pattern/ — always match from line 1; stop after first match
                if *in_range {
                    if end_re.is_match(line) {
                        *in_range = false;
                    }
                    true
                } else if line_num == 1 {
                    // Start matching from line 1 (0 is virtual start)
                    *in_range = true;
                    // Check if first line already matches end pattern
                    if end_re.is_match(line) {
                        *in_range = false;
                    }
                    true
                } else {
                    false
                }
            }
            _ => self.matches_simple(line_num, total_lines, line),
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
/// This is careful to not split inside s/pattern/replacement/ or { } blocks.
fn split_sed_commands(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut in_subst = false;
    let mut delim_count = 0;
    let mut delim: Option<char> = None;
    let mut escaped = false;
    let mut brace_depth = 0;
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
        } else if c == '{' {
            brace_depth += 1;
        } else if c == '}' {
            brace_depth -= 1;
        } else if c == ';' && brace_depth == 0 {
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

        // Check for step address: first~step
        if let Some(after_tilde) = rest.strip_prefix('~') {
            let end2 = after_tilde
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_tilde.len());
            if end2 > 0 {
                let step: usize = after_tilde[..end2]
                    .parse()
                    .map_err(|_| Error::Execution("sed: invalid step address".to_string()))?;
                return Ok((Some(Address::Step(num, step)), &after_tilde[end2..]));
            }
        }

        // Check for range
        if let Some(rest) = rest.strip_prefix(',') {
            if let Some(after_dollar) = rest.strip_prefix('$') {
                return Ok((Some(Address::Range(num, usize::MAX)), after_dollar));
            }
            // N,/pattern/ range — 0,/pat/ is special (matches first occurrence)
            if let Some(after_slash) = rest.strip_prefix('/') {
                let end2 = after_slash.find('/').ok_or_else(|| {
                    Error::Execution("sed: unterminated address regex".to_string())
                })?;
                let pattern = &after_slash[..end2];
                let regex = Regex::new(pattern)
                    .map_err(|e| Error::Execution(format!("sed: invalid regex: {}", e)))?;
                if num == 0 {
                    return Ok((Some(Address::ZeroRegex(regex)), &after_slash[end2 + 1..]));
                }
                return Ok((
                    Some(Address::LineRegexRange(num, regex)),
                    &after_slash[end2 + 1..],
                ));
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
        let rest = &s[end + 2..];

        // Check for regex range: /start/,/end/ or /start/,$
        if let Some(after_comma) = rest.strip_prefix(',') {
            if let Some(after_dollar) = after_comma.strip_prefix('$') {
                // /pattern/,$ — from regex to end
                return Ok((
                    Some(Address::RegexRange(
                        regex,
                        Regex::new("$^").unwrap(), // Never matches - range goes to end
                    )),
                    after_dollar,
                ));
            }
            if let Some(after_slash) = after_comma.strip_prefix('/') {
                let end2 = after_slash.find('/').ok_or_else(|| {
                    Error::Execution("sed: unterminated address regex".to_string())
                })?;
                let pattern2 = &after_slash[..end2];
                let regex2 = Regex::new(pattern2)
                    .map_err(|e| Error::Execution(format!("sed: invalid regex: {}", e)))?;
                return Ok((
                    Some(Address::RegexRange(regex, regex2)),
                    &after_slash[end2 + 1..],
                ));
            }
            // /pattern/,N
            let end2 = after_comma
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_comma.len());
            if end2 > 0 {
                if let Ok(line_num) = after_comma[..end2].parse::<usize>() {
                    // Create regex that matches at line N (handled as RegexRange with a line check)
                    return Ok((
                        Some(Address::Range(0, line_num)), // Approximate
                        &after_comma[end2..],
                    ));
                }
            }
        }

        return Ok((Some(Address::Regex(regex)), rest));
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
            // In BRE mode: \( \) are groups, ( ) are literal, \+ \? are quantifiers
            // In ERE mode: ( ) are groups, + ? work directly
            let pattern = if extended_regex {
                // ERE mode: no conversion needed for groups/quantifiers
                pattern.clone()
            } else {
                // BRE mode: proper char-by-char conversion
                bre_to_ere(pattern)
            };
            // Build regex with optional case-insensitive flag
            let case_insensitive = flags.contains('i');
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(case_insensitive)
                .build()
                .map_err(|e| Error::Execution(format!("sed: invalid pattern: {}", e)))?;

            // Convert sed replacement syntax to regex replacement syntax
            // sed uses \1, \2, etc. and & for full match
            // regex crate uses ${N} format to avoid ambiguity
            let replacement = replacement
                .replace("\\&", "\x00") // Temporarily escape literal &
                .replace('&', "${0}")
                .replace("\x00", "&");

            // Use ${N} format instead of $N to avoid ambiguity with following chars
            let replacement = Regex::new(r"\\(\d+)")
                .unwrap()
                .replace_all(&replacement, r"$${$1}")
                .to_string();

            // Convert \n → newline, \t → tab in replacement
            let replacement = replacement.replace("\\n", "\n").replace("\\t", "\t");

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
        'c' => {
            // Change command: c\text
            let text = if rest.len() > 1 && rest.chars().nth(1) == Some('\\') {
                rest[2..].to_string()
            } else {
                rest[1..].to_string()
            };
            Ok((address, negate, SedCommand::Change(text)))
        }
        'h' => Ok((address, negate, SedCommand::HoldCopy)),
        'H' => Ok((address, negate, SedCommand::HoldAppend)),
        'g' if rest.len() == 1 || !rest[1..].starts_with('/') => {
            // 'g' alone is get-from-hold; 'g' after s// is global flag (handled in Substitute)
            Ok((address, negate, SedCommand::GetCopy))
        }
        'G' => Ok((address, negate, SedCommand::GetAppend)),
        'x' => Ok((address, negate, SedCommand::Exchange)),
        'Q' => Ok((address, negate, SedCommand::QuitNoprint)),
        ':' => {
            // Label: :name
            let label = rest[1..].trim().to_string();
            Ok((None, false, SedCommand::Label(label)))
        }
        'b' => {
            // Branch: b [label] — unconditional jump
            let label = rest[1..].trim();
            let label = if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            };
            Ok((address, negate, SedCommand::Branch(label)))
        }
        't' => {
            // Branch if substitution: t [label]
            let label = rest[1..].trim();
            let label = if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            };
            Ok((address, negate, SedCommand::BranchIfSub(label)))
        }
        '{' => {
            // Grouped commands: { cmd1; cmd2; ... }
            // Find matching closing brace
            let inner = rest[1..].trim();
            let inner = inner.strip_suffix('}').unwrap_or(inner);
            let mut group_cmds = Vec::new();
            for cmd_str in split_sed_commands(inner) {
                let trimmed = cmd_str.trim();
                if !trimmed.is_empty() {
                    let (a, n, c) = parse_sed_command(trimmed, extended_regex)?;
                    group_cmds.push((a, n, c));
                }
            }
            Ok((address, negate, SedCommand::Group(group_cmds)))
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

/// Mutable state passed through command execution
struct LineState {
    current_line: String,
    should_print: bool,
    deleted: bool,
    extra_output: Vec<String>, // lines printed by 'p' command (printed immediately)
    insert_text: Option<String>,
    append_text: Option<String>,
    quit: bool,
    quit_noprint: bool,
    hold_space: String,
    sub_happened: bool, // track if any substitution succeeded (for t command)
}

/// Execute a single sed command against the current line state.
/// Returns true if the command was applied (for branching logic).
fn exec_sed_cmd(cmd: &SedCommand, state: &mut LineState, _line_num: usize, _total_lines: usize) {
    match cmd {
        SedCommand::Substitute {
            pattern,
            replacement,
            global,
            nth,
            print_only,
        } => {
            let new_line = if *global {
                pattern.replace_all(&state.current_line, replacement.as_str())
            } else if let Some(n) = nth {
                replace_nth(pattern, &state.current_line, replacement, *n)
            } else {
                pattern.replace(&state.current_line, replacement.as_str())
            };

            if new_line != state.current_line {
                state.current_line = new_line.into_owned();
                state.sub_happened = true;
                if *print_only {
                    state.extra_output.push(state.current_line.clone());
                }
            }
        }
        SedCommand::Delete => {
            state.deleted = true;
            state.should_print = false;
        }
        SedCommand::Print => {
            // Print current pattern space immediately (snapshot at this point)
            state.extra_output.push(state.current_line.clone());
        }
        SedCommand::Quit => {
            state.quit = true;
        }
        SedCommand::QuitNoprint => {
            state.quit_noprint = true;
        }
        SedCommand::Append(text) => {
            state.append_text = Some(text.clone());
        }
        SedCommand::Insert(text) => {
            state.insert_text = Some(text.clone());
        }
        SedCommand::Change(text) => {
            state.current_line = text.clone();
            state.deleted = false;
            state.should_print = true;
        }
        SedCommand::HoldCopy => {
            state.hold_space = state.current_line.clone();
        }
        SedCommand::HoldAppend => {
            state.hold_space.push('\n');
            state.hold_space.push_str(&state.current_line);
        }
        SedCommand::GetCopy => {
            state.current_line = state.hold_space.clone();
        }
        SedCommand::GetAppend => {
            state.current_line.push('\n');
            state.current_line.push_str(&state.hold_space);
        }
        SedCommand::Exchange => {
            std::mem::swap(&mut state.current_line, &mut state.hold_space);
        }
        SedCommand::Group(cmds) => {
            for (_addr, _negate, sub_cmd) in cmds {
                exec_sed_cmd(sub_cmd, state, _line_num, _total_lines);
                if state.deleted || state.quit || state.quit_noprint {
                    break;
                }
            }
        }
        // Labels and branches are handled at the top-level command loop
        SedCommand::Label(_) | SedCommand::Branch(_) | SedCommand::BranchIfSub(_) => {}
    }
}

/// Count total commands (including nested) for range state tracking
fn count_commands(cmds: &[(Option<Address>, bool, SedCommand)]) -> usize {
    let mut count = 0;
    for (_, _, cmd) in cmds {
        count += 1;
        if let SedCommand::Group(sub) = cmd {
            count += count_commands(sub);
        }
    }
    count
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
            let mut hold_space = String::new();
            let mut global_quit = false;
            // Track range state per command
            let mut range_state: Vec<bool> = vec![false; count_commands(&opts.commands)];

            for (idx, line) in lines.iter().enumerate() {
                if global_quit {
                    break;
                }

                let line_num = idx + 1;
                let mut state = LineState {
                    current_line: line.to_string(),
                    should_print: !opts.quiet,
                    deleted: false,
                    extra_output: Vec::new(),
                    insert_text: None,
                    append_text: None,
                    quit: false,
                    quit_noprint: false,
                    hold_space: hold_space.clone(),
                    sub_happened: false,
                };

                // Execute commands with branch/label support
                let mut cmd_idx = 0;
                let max_iterations = 1000; // prevent infinite loops
                let mut iterations = 0;
                while cmd_idx < opts.commands.len() && iterations < max_iterations {
                    iterations += 1;
                    let (addr, negate, cmd) = &opts.commands[cmd_idx];

                    let addr_matches = addr
                        .as_ref()
                        .map(|a| {
                            a.matches_with_state(
                                line_num,
                                total_lines,
                                &state.current_line,
                                &mut range_state[cmd_idx],
                            )
                        })
                        .unwrap_or(true);

                    let should_apply = if *negate { !addr_matches } else { addr_matches };

                    if !should_apply {
                        cmd_idx += 1;
                        continue;
                    }

                    match cmd {
                        SedCommand::Label(_) => {
                            // Labels are just markers, skip
                            cmd_idx += 1;
                        }
                        SedCommand::Branch(label) => {
                            if let Some(label) = label {
                                // Jump to label
                                if let Some(pos) = find_label(&opts.commands, label) {
                                    cmd_idx = pos;
                                } else {
                                    cmd_idx += 1;
                                }
                            } else {
                                // b with no label = jump to end (skip remaining)
                                break;
                            }
                        }
                        SedCommand::BranchIfSub(label) => {
                            if state.sub_happened {
                                state.sub_happened = false;
                                if let Some(label) = label {
                                    if let Some(pos) = find_label(&opts.commands, label) {
                                        cmd_idx = pos;
                                    } else {
                                        cmd_idx += 1;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                cmd_idx += 1;
                            }
                        }
                        _ => {
                            exec_sed_cmd(cmd, &mut state, line_num, total_lines);
                            cmd_idx += 1;
                        }
                    }

                    if state.deleted || state.quit || state.quit_noprint {
                        break;
                    }
                }

                hold_space = state.hold_space;

                // Insert text comes before the line
                if let Some(text) = state.insert_text {
                    file_output.push_str(&text);
                    file_output.push('\n');
                }

                if state.quit_noprint {
                    global_quit = true;
                    // Q does NOT print the current line
                } else {
                    // Extra output from p command comes before auto-print
                    for extra in &state.extra_output {
                        file_output.push_str(extra);
                        file_output.push('\n');
                    }

                    if !state.deleted && state.should_print {
                        file_output.push_str(&state.current_line);
                        file_output.push('\n');
                    }

                    // Append text comes after the line
                    if let Some(text) = state.append_text {
                        file_output.push_str(&text);
                        file_output.push('\n');
                    }

                    if state.quit {
                        global_quit = true;
                    }
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

/// Find the index of a label command in the command list
fn find_label(cmds: &[(Option<Address>, bool, SedCommand)], target: &str) -> Option<usize> {
    for (i, (_, _, cmd)) in cmds.iter().enumerate() {
        if let SedCommand::Label(name) = cmd {
            if name == target {
                return Some(i);
            }
        }
    }
    None
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
            #[cfg(feature = "git")]
            git_client: None,
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

    // === Hold space tests ===

    #[tokio::test]
    async fn test_sed_hold_copy() {
        // h copies pattern to hold, g retrieves it
        let result = run_sed(&["-e", "1h", "-e", "2g"], Some("first\nsecond\nthird"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "first\nfirst\nthird\n");
    }

    #[tokio::test]
    async fn test_sed_hold_append() {
        // H appends to hold, G appends hold to pattern
        let result = run_sed(&["-e", "1h", "-e", "2H", "-e", "3G"], Some("a\nb\nc"))
            .await
            .unwrap();
        // Line 1: prints "a", holds "a"
        // Line 2: prints "b", hold becomes "a\nb"
        // Line 3: prints "c\na\nb" (G appends hold)
        assert_eq!(result.stdout, "a\nb\nc\na\nb\n");
    }

    #[tokio::test]
    async fn test_sed_exchange() {
        // x swaps pattern and hold
        let result = run_sed(&["-e", "1h", "-e", "2x"], Some("first\nsecond\nthird"))
            .await
            .unwrap();
        // Line 1: prints "first", hold = "first"
        // Line 2: exchange => pattern = "first", hold = "second", prints "first"
        // Line 3: prints "third"
        assert_eq!(result.stdout, "first\nfirst\nthird\n");
    }

    #[tokio::test]
    async fn test_sed_change_command() {
        let result = run_sed(&["2c\\replaced"], Some("line1\nline2\nline3"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "line1\nreplaced\nline3\n");
    }

    #[tokio::test]
    async fn test_sed_regex_range() {
        let result = run_sed(
            &["/start/,/end/d"],
            Some("before\nstart\nmiddle\nend\nafter"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "before\nafter\n");
    }

    #[tokio::test]
    async fn test_sed_regex_range_substitute() {
        let result = run_sed(
            &["/begin/,/end/s/x/y/g"],
            Some("ax\nbeginx\nmiddlex\nendx\nax"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "ax\nbeginy\nmiddley\nendy\nax\n");
    }
}
