//! SQL / dot-command splitter.
//!
//! We accept either a single `;`-separated SQL batch or a sequence of
//! dot-commands (lines starting with `.`). The two cannot mix on a single
//! line — a dot-command must be on its own line. Inside a SQL statement
//! we honour string literals and comments so that `;` inside a `'...'`
//! string is not treated as a terminator.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stmt {
    /// A SQL statement (without trailing `;`). May be empty if the caller
    /// trims and skips empties.
    Sql(String),
    /// A dot-command including its leading `.`, with whitespace-trimmed args.
    Dot(String),
}

/// Split a script into a list of statements. Dot-commands and SQL can be
/// interleaved at line granularity; SQL spans multiple lines until a `;`
/// terminator outside of strings/comments.
pub(super) fn split(script: &str) -> Vec<Stmt> {
    let mut out = Vec::new();
    let mut buf = String::new();
    // We iterate line by line but operate character-by-character within
    // SQL accumulation so that `;` inside literals is honoured.
    for raw_line in script.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if buf.is_empty() && trimmed.starts_with('.') {
            // Dot-command — entire trimmed line, ignoring trailing `;`.
            let dot = trimmed.trim_end().trim_end_matches(';').trim().to_string();
            if !dot.is_empty() && dot != "." {
                out.push(Stmt::Dot(dot));
            }
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
        // Walk current accumulated buffer to find unquoted `;` terminators.
        flush_terminated(&mut buf, &mut out);
    }
    let leftover = buf.trim();
    if !leftover.is_empty() {
        out.push(Stmt::Sql(leftover.to_string()));
    }
    out
}

fn flush_terminated(buf: &mut String, out: &mut Vec<Stmt>) {
    loop {
        let Some(end) = find_unquoted_semicolon(buf) else {
            return;
        };
        let stmt: String = buf.drain(..=end).collect();
        // strip trailing `;` and whitespace
        let trimmed = stmt.trim_end_matches(';').trim().to_string();
        if !trimmed.is_empty() {
            out.push(Stmt::Sql(trimmed));
        }
    }
}

/// Find the next `;` outside of string literals and comments. Returns the
/// byte index of the `;`, or `None` if none is present.
fn find_unquoted_semicolon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\'' => {
                i = skip_quoted(bytes, i, b'\'');
            }
            b'"' => {
                i = skip_quoted(bytes, i, b'"');
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                // line comment to end of line
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // block comment
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b';' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn skip_quoted(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            // SQL doubles the quote to escape: ''
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// Tokenise a dot-command into `(name, args)`.
/// Quoting: a single argument may be wrapped in `'...'` or `"..."`; otherwise
/// whitespace separates arguments.
pub(super) fn tokenize_dot(line: &str) -> (String, Vec<String>) {
    let line = line.strip_prefix('.').unwrap_or(line).trim();
    let mut name = String::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            break;
        }
        name.push(c);
        chars.next();
    }
    let rest: String = chars.collect();
    let args = split_args(rest.trim());
    (name, args)
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                chars.next();
            }
            '\'' | '"' => {
                let quote = c;
                chars.next();
                while let Some(&q) = chars.peek() {
                    if q == quote {
                        chars.next();
                        break;
                    }
                    cur.push(q);
                    chars.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            _ => {
                cur.push(c);
                chars.next();
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_simple_semicolons() {
        let s = split("SELECT 1; SELECT 2;");
        assert_eq!(
            s,
            vec![Stmt::Sql("SELECT 1".into()), Stmt::Sql("SELECT 2".into()),]
        );
    }

    #[test]
    fn keeps_semicolon_inside_string_literal() {
        let s = split("INSERT INTO t VALUES ('a;b'); SELECT 1");
        assert_eq!(
            s,
            vec![
                Stmt::Sql("INSERT INTO t VALUES ('a;b')".into()),
                Stmt::Sql("SELECT 1".into()),
            ]
        );
    }

    #[test]
    fn handles_doubled_quote_escape() {
        // Bobby Tables — `'O''Brien'` is a single SQL token.
        let s = split("INSERT INTO t VALUES ('O''Brien;'); SELECT 1;");
        assert_eq!(
            s,
            vec![
                Stmt::Sql("INSERT INTO t VALUES ('O''Brien;')".into()),
                Stmt::Sql("SELECT 1".into()),
            ]
        );
    }

    #[test]
    fn ignores_semicolon_inside_line_comment() {
        let s = split("SELECT 1 -- ; in comment\n; SELECT 2;");
        assert_eq!(
            s,
            vec![
                Stmt::Sql("SELECT 1 -- ; in comment".into()),
                Stmt::Sql("SELECT 2".into()),
            ]
        );
    }

    #[test]
    fn ignores_semicolon_inside_block_comment() {
        let s = split("SELECT 1 /* ; */ + 2; SELECT 3;");
        assert_eq!(
            s,
            vec![
                Stmt::Sql("SELECT 1 /* ; */ + 2".into()),
                Stmt::Sql("SELECT 3".into()),
            ]
        );
    }

    #[test]
    fn dot_commands_separate_from_sql() {
        let s = split(".tables\nSELECT 1;\n.schema");
        assert_eq!(
            s,
            vec![
                Stmt::Dot(".tables".into()),
                Stmt::Sql("SELECT 1".into()),
                Stmt::Dot(".schema".into()),
            ]
        );
    }

    #[test]
    fn unterminated_sql_kept_as_last_stmt() {
        let s = split("SELECT 1");
        assert_eq!(s, vec![Stmt::Sql("SELECT 1".into())]);
    }

    #[test]
    fn empty_script_returns_empty() {
        assert!(split("").is_empty());
        assert!(split("   \n   ").is_empty());
        assert!(split(";;;").is_empty());
    }

    #[test]
    fn tokenize_dot_basic() {
        let (n, a) = tokenize_dot(".mode csv");
        assert_eq!(n, "mode");
        assert_eq!(a, vec!["csv".to_string()]);
    }

    #[test]
    fn tokenize_dot_quoted_arg() {
        let (n, a) = tokenize_dot(".separator '|'");
        assert_eq!(n, "separator");
        assert_eq!(a, vec!["|".to_string()]);
    }

    #[test]
    fn tokenize_dot_no_args() {
        let (n, a) = tokenize_dot(".tables");
        assert_eq!(n, "tables");
        assert!(a.is_empty());
    }

    #[test]
    fn unterminated_string_does_not_loop() {
        // A pathological input (missing closing quote) must not hang.
        let s = split("SELECT '");
        // We treat it as one unterminated SQL; the engine will reject it.
        assert_eq!(s.len(), 1);
    }
}
