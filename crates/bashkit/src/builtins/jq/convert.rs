//! JSON <-> jaq Val conversion + JSON depth check.
//!
//! Important decisions:
//!  - `Val::Num` Display string is parsed back to JSON to preserve the
//!    exact textual representation (e.g. `1.0` stays `1.0`, not `1`).
//!    This matches real jq's number-formatting parity. We avoid serde_json's
//!    `arbitrary_precision` feature because it changes Number semantics
//!    crate-wide; instead we route number tokens through a custom
//!    `RawNumber` wrapper that's only consulted when serializing jq output.
//!  - `MAX_JQ_JSON_DEPTH` (TM-DOS-027) bounds input nesting to prevent
//!    stack overflow during jaq evaluation on deeply nested JSON.

use jaq_json::Val;

/// THREAT[TM-DOS-027]: Maximum nesting depth for JSON input values.
/// Prevents stack overflow when jaq evaluates deeply nested JSON structures
/// like `[[[[...]]]]` or `{"a":{"a":{"a":...}}}`.
pub(super) const MAX_JQ_JSON_DEPTH: usize = 100;

/// Tagged JSON value used internally by the jq builtin so we can preserve
/// the original numeric representation through filter execution and
/// output formatting. Real jq prints `1.0` as `1.0`; the stock
/// `serde_json::Value` (without `arbitrary_precision`) cannot.
#[derive(Debug, Clone)]
pub(super) enum JqJson {
    Null,
    Bool(bool),
    /// Original token from the input or jaq's Display, e.g. "1", "1.0",
    /// "-3.14", "1e2". Validated as JSON-shaped before construction.
    Number(String),
    String(String),
    Array(Vec<JqJson>),
    Object(Vec<(String, JqJson)>),
}

impl JqJson {
    pub(super) fn is_null(&self) -> bool {
        matches!(self, JqJson::Null)
    }

    pub(super) fn is_false(&self) -> bool {
        matches!(self, JqJson::Bool(false))
    }
}

/// Convert serde_json::Value to our JqJson, capturing original number tokens.
/// THREAT[TM-DOS-027]: depth checked at the same time.
pub(super) fn serde_to_jq(
    v: &serde_json::Value,
    depth: usize,
    max: usize,
) -> std::result::Result<JqJson, String> {
    if depth > max {
        return Err(format!(
            "jq: JSON nesting too deep ({depth} levels, max {max})"
        ));
    }
    Ok(match v {
        serde_json::Value::Null => JqJson::Null,
        serde_json::Value::Bool(b) => JqJson::Bool(*b),
        serde_json::Value::Number(n) => JqJson::Number(n.to_string()),
        serde_json::Value::String(s) => JqJson::String(s.clone()),
        serde_json::Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(serde_to_jq(item, depth + 1, max)?);
            }
            JqJson::Array(out)
        }
        serde_json::Value::Object(map) => {
            let mut out = Vec::with_capacity(map.len());
            for (k, item) in map {
                out.push((k.clone(), serde_to_jq(item, depth + 1, max)?));
            }
            JqJson::Object(out)
        }
    })
}

/// Convert our JqJson to a jaq Val for filter execution.
pub(super) fn jq_to_val(v: &JqJson) -> Val {
    match v {
        JqJson::Null => Val::Null,
        JqJson::Bool(b) => Val::from(*b),
        JqJson::Number(s) => {
            // Try integer first (preserves precision for big-but-fits ints),
            // then fall through to f64.
            if let Ok(i) = s.parse::<i64>()
                && let Ok(i) = isize::try_from(i)
            {
                return Val::from(i);
            }
            if let Ok(f) = s.parse::<f64>() {
                return Val::from(f);
            }
            Val::from(0isize)
        }
        JqJson::String(s) => Val::from(s.clone()),
        JqJson::Array(arr) => arr.iter().map(jq_to_val).collect(),
        JqJson::Object(map) => Val::obj(
            map.iter()
                .map(|(k, v)| (Val::from(k.clone()), jq_to_val(v)))
                .collect(),
        ),
    }
}

/// Convert jaq Val back to JqJson for output formatting. Captures number
/// representation via Val's Display (jaq preserves the original token).
pub(super) fn val_to_jq(v: &Val) -> JqJson {
    match v {
        Val::Null => JqJson::Null,
        Val::Bool(b) => JqJson::Bool(*b),
        Val::Num(_) => {
            // jaq's Num Display preserves the original textual form
            // (e.g. "1.0" stays "1.0"). We capture that token directly.
            let s = format!("{v}");
            // Validate it parses as JSON so downstream output stays well-formed.
            if serde_json::from_str::<serde_json::Value>(&s).is_ok() {
                JqJson::Number(s)
            } else {
                // Defensive fallback for any jaq numeric Display we don't
                // recognise — emit a JSON-safe form.
                if let Ok(f) = s.parse::<f64>() {
                    if f.is_finite() {
                        JqJson::Number(format_f64_canonical(f))
                    } else {
                        JqJson::Null
                    }
                } else {
                    JqJson::Null
                }
            }
        }
        Val::BStr(_) | Val::TStr(_) => {
            // Val's Display wraps strings in quotes — round-trip through JSON
            // to unescape. Falls back to the raw display for unparseable forms.
            let displayed = format!("{v}");
            match serde_json::from_str::<String>(&displayed) {
                Ok(s) => JqJson::String(s),
                Err(_) => JqJson::String(displayed),
            }
        }
        Val::Arr(a) => JqJson::Array(a.iter().map(val_to_jq).collect()),
        Val::Obj(o) => {
            let map: Vec<(String, JqJson)> = o
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Val::TStr(_) | Val::BStr(_) => {
                            let s = format!("{k}");
                            serde_json::from_str::<String>(&s).unwrap_or(s)
                        }
                        _ => format!("{k}"),
                    };
                    (key, val_to_jq(v))
                })
                .collect();
            JqJson::Object(map)
        }
    }
}

/// Format an f64 as JSON, ensuring whole numbers keep `.0` so they remain
/// parseable as floats by downstream readers (matches Rust's f64 Debug).
fn format_f64_canonical(f: f64) -> String {
    let s = format!("{f}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// THREAT[TM-DOS-027]: Standalone depth check used by --argjson and other
/// entry points where the value comes from outside the main parser.
pub(super) fn check_json_depth(
    value: &serde_json::Value,
    max_depth: usize,
) -> std::result::Result<(), String> {
    fn measure(v: &serde_json::Value, cur: usize, max: usize) -> std::result::Result<(), String> {
        if cur > max {
            return Err(format!(
                "jq: JSON nesting too deep ({cur} levels, max {max})"
            ));
        }
        match v {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    measure(item, cur + 1, max)?;
                }
            }
            serde_json::Value::Object(map) => {
                for (_k, item) in map {
                    measure(item, cur + 1, max)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    measure(value, 0, max_depth)
}

/// Parse multiple JSON values from a stream (handles NDJSON, multi-line,
/// concatenated). Each value is depth-checked.
pub(super) fn parse_json_stream(input: &str) -> std::result::Result<Vec<JqJson>, String> {
    use serde_json::Deserializer;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut vals = Vec::new();
    let stream = Deserializer::from_str(trimmed).into_iter::<serde_json::Value>();
    for result in stream {
        let v = result.map_err(|e| format!("jq: invalid JSON: {e}"))?;
        vals.push(serde_to_jq(&v, 0, MAX_JQ_JSON_DEPTH)?);
    }
    Ok(vals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_float_zero_decimal() {
        // 1.0 must NOT collapse to 1 — real jq preserves it.
        let v = serde_json::json!(1.0);
        let jq = serde_to_jq(&v, 0, 100).unwrap();
        match jq {
            JqJson::Number(s) => assert_eq!(s, "1.0"),
            _ => panic!("expected Number"),
        }
    }

    #[test]
    fn integer_stays_integer() {
        let v = serde_json::json!(42);
        let jq = serde_to_jq(&v, 0, 100).unwrap();
        match jq {
            JqJson::Number(s) => assert_eq!(s, "42"),
            _ => panic!("expected Number"),
        }
    }

    #[test]
    fn check_json_depth_flat_ok() {
        let v = serde_json::json!(42);
        assert!(check_json_depth(&v, 100).is_ok());
    }

    #[test]
    fn check_json_depth_nested_ok() {
        let v = serde_json::json!([[[1]]]);
        assert!(check_json_depth(&v, 5).is_ok());
    }

    #[test]
    fn check_json_depth_too_deep() {
        let v = serde_json::json!([[[1]]]);
        assert!(check_json_depth(&v, 2).is_err());
    }

    #[test]
    fn parse_json_stream_handles_ndjson() {
        let result = parse_json_stream("1\n2\n3").unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn parse_json_stream_empty() {
        let result = parse_json_stream("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_json_stream_rejects_deep() {
        let depth = 150;
        let s = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
        // serde_json itself caps at ~128, so either error path is acceptable.
        let result = parse_json_stream(&s);
        assert!(result.is_err());
    }
}
