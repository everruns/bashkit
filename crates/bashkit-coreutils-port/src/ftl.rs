//! Fluent (`.ftl`) message parsing, shared by args mode and module mode.
//!
//! Both modes resolve uucore's `translate!()` family **at port time**, so
//! bashkit never links Fluent or ships locale bundles (see
//! `knowledge/runtimes/coreutils-args-port.md`). They need the same two
//! things, hence this module rather than a copy in each:
//!
//! 1. [`parse_ftl`] — the flat `key = value` (+ indented continuation)
//!    subset of Fluent that uucore uses for help strings and error text.
//! 2. [`parse_pattern`] — split one message body into literal text and
//!    `{ $var }` placeable slots, so module mode can fold a message with
//!    arguments into a `format!()` call.
//!
//! Deliberately *not* a Fluent implementation. Selectors, plurals, term
//! references, and attributes are constructs whose rendering depends on
//! runtime locale data; silently mis-rendering them would bake wrong user
//! text into generated code. [`parse_pattern`] reports them as
//! [`PatternError::Unsupported`] so the caller aborts the port instead.

use std::collections::HashMap;

/// Parse a Fluent file restricted to the `key = value` and continuation
/// subset that uutils uses for help/about strings.
///
/// Supported:
///   `key = value`
///   `  continuation line`
///
/// Values are returned raw: any `{ … }` placeables are left in place for
/// [`parse_pattern`] to interpret (args mode only ever substitutes keys
/// whose value has none).
pub fn parse_ftl(src: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_val: String = String::new();

    let flush = |out: &mut HashMap<String, String>, key: &mut Option<String>, val: &mut String| {
        if let Some(k) = key.take() {
            out.insert(k, std::mem::take(val).trim_end().to_string());
        }
    };

    for raw_line in src.lines() {
        let line = raw_line;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            flush(&mut out, &mut current_key, &mut current_val);
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            // continuation
            if current_key.is_some() {
                current_val.push('\n');
                current_val.push_str(line.trim_start());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            flush(&mut out, &mut current_key, &mut current_val);
            current_key = Some(k.trim().to_string());
            current_val = v.trim().to_string();
        }
    }
    flush(&mut out, &mut current_key, &mut current_val);
    out
}

/// One piece of a parsed message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Literal text, already unescaped.
    Text(String),
    /// A `{ $name }` variable slot; `name` excludes the `$`.
    Placeable(String),
}

/// Why a message body could not be reduced to text + variable slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// A `{ … }` slot that is not a bare `$variable` (selector, term
    /// reference, message reference, nested expression, literal…).
    Unsupported(String),
    /// A `{` with no matching `}`.
    Unterminated,
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternError::Unsupported(inner) => write!(
                f,
                "unsupported Fluent placeable '{{{inner}}}' (only bare '{{ $var }}' slots resolve at port time)"
            ),
            PatternError::Unterminated => write!(f, "unterminated '{{' in Fluent message"),
        }
    }
}

/// Split a Fluent message body into literal text and `{ $var }` slots.
///
/// `\{` and `\}` are treated as escaped literal braces, matching how
/// uucore's own message files spell a literal brace next to a placeable
/// (e.g. `invalid universal character name \{ $escape }`, where the
/// backslash is user-visible text preceding the slot).
pub fn parse_pattern(body: &str) -> Result<Vec<Segment>, PatternError> {
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut chars = body.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if matches!(chars.peek(), Some('{') | Some('}')) => {
                // Fluent has no text-level backslash escape: a `\` before a
                // placeable is literal text the user is meant to see. Keep
                // both the backslash and let the brace start a slot.
                text.push('\\');
            }
            '{' => {
                let mut inner = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    inner.push(c);
                }
                if !closed {
                    return Err(PatternError::Unterminated);
                }
                let trimmed = inner.trim();
                let Some(var) = trimmed.strip_prefix('$') else {
                    return Err(PatternError::Unsupported(inner));
                };
                let var = var.trim();
                if var.is_empty()
                    || !var
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(PatternError::Unsupported(inner));
                }
                if !text.is_empty() {
                    segments.push(Segment::Text(std::mem::take(&mut text)));
                }
                segments.push(Segment::Placeable(var.to_string()));
            }
            other => text.push(other),
        }
    }

    if !text.is_empty() {
        segments.push(Segment::Text(text));
    }
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_keys_and_continuations() {
        let map = parse_ftl("a = one\nb = two\n  and more\n\n# comment\nc = three\n");
        assert_eq!(map["a"], "one");
        assert_eq!(map["b"], "two\nand more");
        assert_eq!(map["c"], "three");
    }

    #[test]
    fn pattern_without_placeables_is_one_text_segment() {
        assert_eq!(
            parse_pattern("no more arguments").unwrap(),
            vec![Segment::Text("no more arguments".into())]
        );
    }

    #[test]
    fn pattern_splits_placeables() {
        assert_eq!(
            parse_pattern("format '{ $format }' has no % directive").unwrap(),
            vec![
                Segment::Text("format '".into()),
                Segment::Placeable("format".into()),
                Segment::Text("' has no % directive".into()),
            ]
        );
    }

    #[test]
    fn pattern_keeps_backslash_before_placeable() {
        // uucore spells GNU's `Ø` diagnostic this way.
        assert_eq!(
            parse_pattern("invalid universal character name \\{ $escape }{ $digits }").unwrap(),
            vec![
                Segment::Text("invalid universal character name \\".into()),
                Segment::Placeable("escape".into()),
                Segment::Placeable("digits".into()),
            ]
        );
    }

    #[test]
    fn pattern_leading_placeable() {
        assert_eq!(
            parse_pattern("%{ $spec }: invalid conversion specification").unwrap(),
            vec![
                Segment::Text("%".into()),
                Segment::Placeable("spec".into()),
                Segment::Text(": invalid conversion specification".into()),
            ]
        );
    }

    #[test]
    fn rejects_selector() {
        let err = parse_pattern("{ $n ->\n [one] a\n*[other] b\n}").unwrap_err();
        assert!(matches!(err, PatternError::Unsupported(_)), "got: {err:?}");
    }

    #[test]
    fn rejects_term_reference() {
        assert!(matches!(
            parse_pattern("see { -brand }").unwrap_err(),
            PatternError::Unsupported(_)
        ));
    }

    #[test]
    fn rejects_unterminated_brace() {
        assert_eq!(
            parse_pattern("oops { $x").unwrap_err(),
            PatternError::Unterminated
        );
    }
}
