//! jq error formatting.
//!
//! Real jq prints concise diagnostics like `jq: error: foo/0 is not
//! defined` or `jq: error (at <stdin>:1): Cannot index array with string`.
//! We implement the same shape over jaq's typed errors, hiding internal
//! Debug shapes that would otherwise leak the prepended compat-defs source
//! into stderr (TM-INF-022).

use jaq_core::compile::Undefined;
use jaq_json::Val;

/// Cap for formatted compile/parse error messages. Bounds stderr so jaq
/// internals (file structs, AST debug, ~800 chars of prepended stdlib) never
/// reach the agent — real jq errors are short, ours must be too.
pub(super) const MAX_JQ_DIAG_CHARS: usize = 240;

/// Cap for runtime-error bodies.
pub(super) const MAX_JQ_RUNTIME_ERROR_CHARS: usize = 240;

/// Inline preview of a jq Value in error messages.
pub(super) const MAX_JQ_VALUE_PREVIEW_CHARS: usize = 11;

/// Cap for string-literal previews in errors.
pub(super) const MAX_JQ_STRING_ERROR_CHARS: usize = 80;

/// Format jaq compile errors as jq-style `name/arity is not defined` messages.
/// Hides the underlying `(File, Vec<(name, Undefined)>)` debug shape that
/// would otherwise leak the full prepended compat-defs source into stderr.
pub(super) fn format_compile_errors<P>(errs: jaq_core::compile::Errors<&str, P>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (_file, file_errs) in errs {
        for (name, undef) in file_errs {
            parts.push(match undef {
                Undefined::Filter(arity) => format!("{name}/{arity} is not defined"),
                Undefined::Var => format!("{name} is not defined"),
                Undefined::Label => format!("label {name} is not defined"),
                Undefined::Mod => format!("module {name} is not defined"),
                _ => format!("{} {name} is not defined", undef.as_str()),
            });
        }
    }
    let body = if parts.is_empty() {
        "compile error".to_string()
    } else {
        parts.join(", ")
    };
    format!("jq: error: {}\n", truncate_text(&body, MAX_JQ_DIAG_CHARS))
}

/// Format jaq load (lex/parse/io) errors as short jq-style messages.
/// Skips offending-token text so prepended compat-defs source can't leak.
pub(super) fn format_load_errors<P>(errs: jaq_core::load::Errors<&str, P>) -> String {
    use jaq_core::load::Error as LoadError;
    let mut parts: Vec<String> = Vec::new();
    for (_file, err) in errs {
        match err {
            LoadError::Lex(es) => {
                for (expect, _) in es {
                    parts.push(format!("expected {}", expect.as_str()));
                }
            }
            LoadError::Parse(es) => {
                for (expect, _) in es {
                    parts.push(format!("expected {}", expect.as_str()));
                }
            }
            LoadError::Io(es) => {
                for (name, _) in es {
                    parts.push(format!("could not load module {name}"));
                }
            }
        }
    }
    let body = if parts.is_empty() {
        "syntax error".to_string()
    } else {
        parts.join(", ")
    };
    format!("jq: error: {}\n", truncate_text(&body, MAX_JQ_DIAG_CHARS))
}

/// Format jaq runtime errors without dumping full input values into stderr.
/// stderr is agent-facing API: value-bearing diagnostics summarize operand
/// types and keep generic fallbacks bounded.
pub(super) fn format_runtime_error(error: &jaq_core::Error<Val>) -> String {
    let message = error.to_string();
    let body = humanize(&message)
        .unwrap_or_else(|| capitalize_first(&truncate_text(&message, MAX_JQ_RUNTIME_ERROR_CHARS)));
    format!("jq: error: {body}\n")
}

fn humanize(message: &str) -> Option<String> {
    if let Some(rest) = message.strip_prefix("cannot index ") {
        let (left, rest) = take_value(rest)?;
        let rest = rest.strip_prefix(" with ")?;
        let (right, trailing) = take_value(rest)?;
        if trailing.trim().is_empty() {
            return Some(format!(
                "Cannot index {} with {}",
                summarize_value(left),
                summarize_value(right)
            ));
        }
    }

    if let Some(rest) = message.strip_prefix("cannot use ") {
        let (value, rest) = take_value(rest)?;
        let typ = rest.strip_prefix(" as ")?.trim();
        if !typ.is_empty() {
            if typ.starts_with("iterable") {
                return Some(format!("Cannot iterate over {}", typed_value(value)));
            }
            return Some(format!("Cannot use {} as {typ}", summarize_value(value)));
        }
    }

    if let Some(rest) = message.strip_prefix("cannot calculate ") {
        let (left, rest) = take_value(rest)?;
        let rest = rest.trim_start();
        let (op, rest) = take_operator(rest)?;
        let (right, trailing) = take_value(rest)?;
        if trailing.trim().is_empty() {
            return Some(format!(
                "{} and {} cannot be {}",
                typed_value(left),
                typed_value(right),
                math_operation_name(op)
            ));
        }
    }

    if let Some(value) = message.strip_prefix("invalid path expression with input ") {
        return Some(format!(
            "Invalid path expression with input {}",
            summarize_value(value)
        ));
    }

    None
}

fn take_operator(input: &str) -> Option<(&str, &str)> {
    ["+", "-", "*", "/", "%"]
        .into_iter()
        .find_map(|op| input.strip_prefix(op).map(|rest| (op, rest)))
}

fn math_operation_name(op: &str) -> &'static str {
    match op {
        "+" => "added",
        "-" => "subtracted",
        "*" => "multiplied",
        "/" => "divided",
        "%" => "remaindered",
        _ => "calculated",
    }
}

fn take_value(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let end = value_end(input)?;
    Some((&input[..end], &input[end..]))
}

fn value_end(input: &str) -> Option<usize> {
    match input.chars().next()? {
        '"' => quoted_string_end(input),
        '[' => bracketed_end(input, '[', ']'),
        '{' => bracketed_end(input, '{', '}'),
        _ => input
            .char_indices()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))
            .or(Some(input.len())),
    }
}

fn quoted_string_end(input: &str) -> Option<usize> {
    let mut escaped = false;
    for (idx, ch) in input.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(idx + ch.len_utf8());
        }
    }
    None
}

fn bracketed_end(input: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(idx + ch.len_utf8());
            }
        }
    }

    None
}

fn summarize_value(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with('[') {
        "array".to_string()
    } else if raw.starts_with('{') {
        "object".to_string()
    } else if raw == "null" {
        "null".to_string()
    } else if raw == "true" || raw == "false" {
        "boolean".to_string()
    } else if raw.starts_with('"') {
        format!("string {}", truncate_string_literal(raw))
    } else if raw.parse::<f64>().is_ok() {
        "number".to_string()
    } else {
        truncate_text(raw, MAX_JQ_STRING_ERROR_CHARS)
    }
}

fn typed_value(raw: &str) -> String {
    let raw = raw.trim();
    let typ = if raw.starts_with('[') {
        "array"
    } else if raw.starts_with('{') {
        "object"
    } else if raw.starts_with('"') {
        "string"
    } else if raw == "true" || raw == "false" {
        "boolean"
    } else if raw == "null" {
        "null"
    } else if raw.parse::<f64>().is_ok() {
        "number"
    } else {
        "value"
    };
    format!("{typ} ({})", value_preview(raw))
}

fn value_preview(raw: &str) -> String {
    let raw = raw.trim();
    if raw.chars().count() <= MAX_JQ_VALUE_PREVIEW_CHARS {
        return raw.to_string();
    }
    format!(
        "{}...",
        raw.chars()
            .take(MAX_JQ_VALUE_PREVIEW_CHARS)
            .collect::<String>()
    )
}

fn truncate_string_literal(raw: &str) -> String {
    let Ok(value) = serde_json::from_str::<String>(raw) else {
        return truncate_text(raw, MAX_JQ_STRING_ERROR_CHARS);
    };
    if value.chars().count() <= MAX_JQ_STRING_ERROR_CHARS {
        return raw.to_string();
    }
    let truncated = format!(
        "{}...",
        value
            .chars()
            .take(MAX_JQ_STRING_ERROR_CHARS.saturating_sub(3))
            .collect::<String>()
    );
    serde_json::to_string(&truncated).unwrap_or_else(|_| "\"...\"".to_string())
}

pub(super) fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", input.chars().take(keep).collect::<String>())
}

fn capitalize_first(input: &str) -> String {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_text_short_unchanged() {
        assert_eq!(truncate_text("hello", 10), "hello");
    }

    #[test]
    fn truncate_text_long_truncated() {
        let long = "a".repeat(300);
        let t = truncate_text(&long, 10);
        assert!(t.ends_with("..."));
        assert_eq!(t.chars().count(), 10);
    }

    #[test]
    fn humanize_index_array_with_string() {
        let h = humanize("cannot index [1,2] with \"foo\"").unwrap();
        assert_eq!(h, "Cannot index array with string \"foo\"");
    }

    #[test]
    fn humanize_iterate_over_null() {
        let h = humanize("cannot use null as iterable").unwrap();
        assert_eq!(h, "Cannot iterate over null (null)");
    }

    #[test]
    fn humanize_math_operands() {
        let h = humanize("cannot calculate [1,2] + 1").unwrap();
        assert_eq!(h, "array ([1,2]) and number (1) cannot be added");
    }

    #[test]
    fn humanize_unknown_message_returns_none() {
        assert!(humanize("something completely different").is_none());
    }

    #[test]
    fn capitalize_first_handles_empty() {
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn capitalize_first_uppercases_letter() {
        assert_eq!(capitalize_first("hello"), "Hello");
    }
}
