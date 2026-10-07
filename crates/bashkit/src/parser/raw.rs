//! Helpers for words kept as written in the source.
//!
//! Important decision: bash prints function bodies (`type`, `declare -f`)
//! with each word exactly as written, so the parser records word source text
//! inside function definitions (`Word::raw`). These helpers derive the
//! source-level pieces the parser cannot get from decoded tokens.

/// Quote-remove a here-document delimiter as written; the bool reports
/// whether any part of it was quoted (bash then reads the body literally).
pub(crate) fn heredoc_eof_from_raw(raw: &str) -> (String, bool) {
    let mut eof = String::new();
    let mut quoted = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                quoted = true;
                if let Some(n) = chars.next() {
                    eof.push(n);
                }
            }
            '\'' => {
                quoted = true;
                for n in chars.by_ref() {
                    if n == '\'' {
                        break;
                    }
                    eof.push(n);
                }
            }
            '"' => {
                quoted = true;
                while let Some(n) = chars.next() {
                    match n {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            if let Some(e) = chars.next() {
                                eof.push(e);
                            }
                        }
                        _ => eof.push(n),
                    }
                }
            }
            _ => eof.push(c),
        }
    }
    (eof, quoted)
}

/// Single-quote `s` for shell output (`'it'\''s'`).
pub(crate) fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Split source text on unquoted blanks, keeping quotes, escapes and
/// `$(..)`/`${..}` groups intact (compound array elements as written).
/// Backslash-newline continuations are dropped, as the lexer does.
pub(crate) fn split_raw_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(n) => {
                    cur.push(c);
                    cur.push(n);
                }
                None => cur.push(c),
            },
            '\'' => {
                cur.push(c);
                for n in chars.by_ref() {
                    cur.push(n);
                    if n == '\'' {
                        break;
                    }
                }
            }
            '"' => {
                cur.push(c);
                while let Some(n) = chars.next() {
                    cur.push(n);
                    if n == '\\' {
                        if let Some(e) = chars.next() {
                            cur.push(e);
                        }
                    } else if n == '"' {
                        break;
                    }
                }
            }
            '(' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | '}' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ' ' | '\t' | '\n' if depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
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
    fn heredoc_delimiter_quote_removal() {
        assert_eq!(heredoc_eof_from_raw("EOF"), ("EOF".into(), false));
        assert_eq!(heredoc_eof_from_raw("'EOF'"), ("EOF".into(), true));
        assert_eq!(heredoc_eof_from_raw("\\E\\OF"), ("EOF".into(), true));
        assert_eq!(heredoc_eof_from_raw("E\"OF\""), ("EOF".into(), true));
    }

    #[test]
    fn single_quote_escapes_quotes() {
        assert_eq!(single_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn split_raw_words_keeps_quotes_and_groups() {
        assert_eq!(
            split_raw_words(" 1   2\n 3 "),
            vec!["1".to_string(), "2".into(), "3".into()]
        );
        assert_eq!(
            split_raw_words("'a b' \"c d\" $(x y) e\\ f"),
            vec![
                "'a b'".to_string(),
                "\"c d\"".into(),
                "$(x y)".into(),
                "e\\ f".into()
            ]
        );
    }
}
