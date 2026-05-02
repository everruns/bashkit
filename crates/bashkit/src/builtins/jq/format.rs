//! Output formatting for jq results.
//!
//! Real jq supports `--indent N` (0..=7), `--tab`, `-c` (compact),
//! and `-S` (sort-keys). We re-implement those here over our `JqJson`
//! representation so the original number tokens are preserved (e.g.
//! `1.0` doesn't collapse to `1`).

use super::convert::JqJson;

/// Output indentation choice. `Spaces(0)` matches `-c` semantics in tools
/// that bypass the compact path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Indent {
    /// Compact output, no whitespace between tokens (matches `-c`).
    Compact,
    /// Pretty output with N-space indentation. jq accepts 0..=7; values
    /// outside that range are clamped to the max at parse time.
    Spaces(u8),
    /// Pretty output with tab indentation (matches `--tab`).
    Tab,
}

/// Recursively sort all object keys (matches `-S`).
pub(super) fn sort_keys(v: JqJson) -> JqJson {
    match v {
        JqJson::Object(map) => {
            let mut sorted: Vec<(String, JqJson)> =
                map.into_iter().map(|(k, v)| (k, sort_keys(v))).collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            JqJson::Object(sorted)
        }
        JqJson::Array(arr) => JqJson::Array(arr.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

/// Render a `JqJson` value using the chosen indent style.
///
/// Note: real jq treats `--indent 0` as equivalent to `-c` (compact, no
/// newlines), not "newlines without indentation". We match that.
pub(super) fn render(v: &JqJson, indent: Indent) -> String {
    let mut out = String::new();
    match indent {
        Indent::Compact => render_compact(v, &mut out),
        Indent::Spaces(0) => render_compact(v, &mut out),
        Indent::Spaces(n) => render_pretty(v, &mut out, &" ".repeat(n as usize), 0),
        Indent::Tab => render_pretty(v, &mut out, "\t", 0),
    }
    out
}

fn render_compact(v: &JqJson, out: &mut String) {
    match v {
        JqJson::Null => out.push_str("null"),
        JqJson::Bool(true) => out.push_str("true"),
        JqJson::Bool(false) => out.push_str("false"),
        JqJson::Number(s) => out.push_str(s),
        JqJson::String(s) => write_json_string(s, out),
        JqJson::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                render_compact(item, out);
            }
            out.push(']');
        }
        JqJson::Object(map) => {
            out.push('{');
            for (i, (k, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                render_compact(item, out);
            }
            out.push('}');
        }
    }
}

fn render_pretty(v: &JqJson, out: &mut String, indent: &str, level: usize) {
    match v {
        JqJson::Null => out.push_str("null"),
        JqJson::Bool(true) => out.push_str("true"),
        JqJson::Bool(false) => out.push_str("false"),
        JqJson::Number(s) => out.push_str(s),
        JqJson::String(s) => write_json_string(s, out),
        JqJson::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(out, indent, level + 1);
                render_pretty(item, out, indent, level + 1);
            }
            out.push('\n');
            push_indent(out, indent, level);
            out.push(']');
        }
        JqJson::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(out, indent, level + 1);
                write_json_string(k, out);
                out.push_str(": ");
                render_pretty(item, out, indent, level + 1);
            }
            out.push('\n');
            push_indent(out, indent, level);
            out.push('}');
        }
    }
}

fn push_indent(out: &mut String, indent: &str, level: usize) {
    if indent.is_empty() {
        return;
    }
    for _ in 0..level {
        out.push_str(indent);
    }
}

/// Write a JSON-escaped string (RFC 8259 minimal-escape set).
/// Real jq does not escape forward slash by default; we match that.
fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> JqJson {
        JqJson::Number(s.to_string())
    }

    #[test]
    fn compact_array() {
        let v = JqJson::Array(vec![n("1"), n("2"), n("3")]);
        assert_eq!(render(&v, Indent::Compact), "[1,2,3]");
    }

    #[test]
    fn pretty_two_space() {
        let v = JqJson::Object(vec![
            ("a".into(), n("1")),
            ("b".into(), JqJson::Array(vec![n("2")])),
        ]);
        let out = render(&v, Indent::Spaces(2));
        assert!(out.contains("\n  \"a\": 1"));
        assert!(out.contains("\n    2"));
    }

    #[test]
    fn pretty_four_space() {
        let v = JqJson::Object(vec![("a".into(), n("1"))]);
        let out = render(&v, Indent::Spaces(4));
        assert!(out.contains("\n    \"a\": 1"));
    }

    #[test]
    fn indent_zero_renders_compact() {
        // Real jq: --indent 0 is equivalent to -c (no newlines at all).
        let v = JqJson::Object(vec![("a".into(), n("1"))]);
        let out = render(&v, Indent::Spaces(0));
        assert_eq!(out, "{\"a\":1}");
    }

    #[test]
    fn tab_indent() {
        let v = JqJson::Object(vec![("a".into(), n("1"))]);
        let out = render(&v, Indent::Tab);
        assert!(out.contains("\n\t\"a\": 1"));
    }

    #[test]
    fn empty_array_compact_pretty() {
        let v = JqJson::Array(vec![]);
        assert_eq!(render(&v, Indent::Compact), "[]");
        assert_eq!(render(&v, Indent::Spaces(2)), "[]");
    }

    #[test]
    fn empty_object_compact_pretty() {
        let v = JqJson::Object(vec![]);
        assert_eq!(render(&v, Indent::Compact), "{}");
        assert_eq!(render(&v, Indent::Spaces(2)), "{}");
    }

    #[test]
    fn preserves_float_zero_decimal() {
        let v = n("1.0");
        assert_eq!(render(&v, Indent::Compact), "1.0");
        assert_eq!(render(&v, Indent::Spaces(2)), "1.0");
    }

    #[test]
    fn string_escapes_quotes_and_newline() {
        let v = JqJson::String("a\"b\nc".into());
        assert_eq!(render(&v, Indent::Compact), "\"a\\\"b\\nc\"");
    }

    #[test]
    fn string_does_not_escape_forward_slash() {
        // Real jq leaves "/" unescaped; matches our behavior.
        let v = JqJson::String("/path/to".into());
        assert_eq!(render(&v, Indent::Compact), "\"/path/to\"");
    }

    #[test]
    fn string_escapes_control_chars() {
        let v = JqJson::String("\x01".into());
        assert_eq!(render(&v, Indent::Compact), "\"\\u0001\"");
    }

    #[test]
    fn sort_keys_recursive() {
        let v = JqJson::Object(vec![
            ("b".into(), n("1")),
            (
                "a".into(),
                JqJson::Object(vec![("z".into(), n("9")), ("y".into(), n("8"))]),
            ),
        ]);
        let sorted = sort_keys(v);
        if let JqJson::Object(map) = &sorted {
            assert_eq!(map[0].0, "a");
            assert_eq!(map[1].0, "b");
            if let JqJson::Object(inner) = &map[0].1 {
                assert_eq!(inner[0].0, "y");
                assert_eq!(inner[1].0, "z");
            } else {
                panic!("nested not object");
            }
        } else {
            panic!("not object");
        }
    }
}
