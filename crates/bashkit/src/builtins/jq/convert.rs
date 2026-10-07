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

use super::jaq_json::Val;

/// THREAT[TM-DOS-027]: Maximum nesting depth for JSON input values.
/// Prevents stack overflow when jaq evaluates deeply nested JSON structures
/// like `[[[[...]]]]` or `{"a":{"a":{"a":...}}}`.
pub(super) const MAX_JQ_JSON_DEPTH: usize = 100;

/// Tagged JSON value used internally by the jq builtin so we can preserve
/// the original numeric representation through filter execution and
/// output formatting. Real jq prints `1.0` as `1.0`; the stock
/// `serde_json::Value` (without `arbitrary_precision`) cannot.
#[derive(Debug, Clone, PartialEq)]
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

/// Depth-checking, order-preserving JSON reader for [`JqJson`].
///
/// Real jq keeps object keys in input order, so input never passes through
/// `serde_json::Value` (whose map is sorted without `preserve_order`, a
/// crate-wide feature we do not want to flip). THREAT[TM-DOS-027]: nesting
/// is checked while reading; the first violation is parked in `too_deep` so
/// callers report it verbatim instead of as a generic parse error.
struct JqSeed<'a> {
    depth: usize,
    max: usize,
    too_deep: &'a std::cell::Cell<Option<String>>,
}

impl<'a> JqSeed<'a> {
    fn child(&self) -> JqSeed<'a> {
        JqSeed {
            depth: self.depth + 1,
            max: self.max,
            too_deep: self.too_deep,
        }
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for JqSeed<'_> {
    type Value = JqJson;

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<JqJson, D::Error> {
        if self.depth > self.max {
            let msg = format!(
                "jq: JSON nesting too deep ({} levels, max {})",
                self.depth, self.max
            );
            self.too_deep.set(Some(msg.clone()));
            return Err(serde::de::Error::custom(msg));
        }
        d.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for JqSeed<'_> {
    type Value = JqJson;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<JqJson, E> {
        Ok(JqJson::Null)
    }

    fn visit_bool<E>(self, b: bool) -> Result<JqJson, E> {
        Ok(JqJson::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<JqJson, E> {
        Ok(JqJson::Number(n.to_string()))
    }

    fn visit_u64<E>(self, n: u64) -> Result<JqJson, E> {
        Ok(JqJson::Number(n.to_string()))
    }

    fn visit_f64<E: serde::de::Error>(self, f: f64) -> Result<JqJson, E> {
        // Same token serde_json::Number prints, so `1.0` stays `1.0`.
        serde_json::Number::from_f64(f)
            .map(|n| JqJson::Number(n.to_string()))
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E>(self, s: &str) -> Result<JqJson, E> {
        Ok(JqJson::String(s.to_owned()))
    }

    fn visit_string<E>(self, s: String) -> Result<JqJson, E> {
        Ok(JqJson::String(s))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<JqJson, A::Error> {
        let mut out = Vec::new();
        while let Some(item) = seq.next_element_seed(self.child())? {
            out.push(item);
        }
        Ok(JqJson::Array(out))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<JqJson, A::Error> {
        let mut out: Vec<(String, JqJson)> = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(self.child())?;
            // Duplicate keys: last value wins in its first position, as in jq.
            if let Some(slot) = out.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = value;
            } else {
                out.push((key, value));
            }
        }
        Ok(JqJson::Object(out))
    }
}

/// Parse exactly one JSON value (`--argjson`, `--jsonargs`), keeping key
/// order. `Err` is the user-facing message; depth errors come back verbatim,
/// syntax errors as serde's text (callers add their own prefix).
pub(super) fn parse_json_value(input: &str) -> std::result::Result<JqJson, JsonParseError> {
    use serde::de::DeserializeSeed;
    let too_deep = std::cell::Cell::new(None);
    let mut de = serde_json::Deserializer::from_str(input);
    let seed = JqSeed {
        depth: 0,
        max: MAX_JQ_JSON_DEPTH,
        too_deep: &too_deep,
    };
    let result = seed.deserialize(&mut de).and_then(|v| de.end().map(|()| v));
    result.map_err(|e| match too_deep.take() {
        Some(msg) => JsonParseError::TooDeep(msg),
        None => JsonParseError::Invalid(e.to_string()),
    })
}

/// Why [`parse_json_value`] rejected its input.
#[derive(Debug)]
pub(super) enum JsonParseError {
    /// Nesting past `MAX_JQ_JSON_DEPTH`; the message is complete.
    TooDeep(String),
    /// Malformed JSON; serde's description without a `jq:` prefix.
    Invalid(String),
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
            // Other literals keep their token (`1.0` prints `1.0`, as in jq)
            // until arithmetic turns them into floats.
            if s.parse::<f64>().is_ok() {
                return Val::Num(super::jaq_json::Num::Dec(super::jaq_json::Rc::new(
                    s.clone(),
                )));
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
/// Convert a result for output. `None` when the value would render to more
/// than `max_bytes`: a value built from shared parts (`[., .]` repeated) is
/// small in memory but expands exponentially here (TM-DOS-110).
pub(super) fn val_to_jq_capped(v: &Val, max_bytes: usize) -> Option<JqJson> {
    let mut budget = max_bytes;
    val_to_jq_budget(v, &mut budget)
}

/// Every node costs at least one output byte, strings their length.
fn val_to_jq_budget(v: &Val, budget: &mut usize) -> Option<JqJson> {
    let cost = match v {
        Val::BStr(b) | Val::TStr(b) => b.len().max(1),
        _ => 1,
    };
    *budget = budget.checked_sub(cost)?;
    Some(match v {
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
        Val::Arr(a) => JqJson::Array(
            a.iter()
                .map(|x| val_to_jq_budget(x, budget))
                .collect::<Option<_>>()?,
        ),
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
                    Some((key, val_to_jq_budget(v, budget)?))
                })
                .collect::<Option<_>>()?;
            JqJson::Object(map)
        }
    })
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

/// Parse multiple JSON values from a stream (handles NDJSON, multi-line,
/// concatenated). Each value is depth-checked and keeps its key order.
pub(super) fn parse_json_stream(input: &str) -> std::result::Result<Vec<JqJson>, String> {
    match parse_json_stream_partial(input) {
        (vals, None) => Ok(vals),
        (_, Some(e)) => Err(e),
    }
}

/// Parse a whitespace-separated JSON stream, returning the values before
/// the first syntax error together with that error.
pub(super) fn parse_json_stream_partial(input: &str) -> (Vec<JqJson>, Option<String>) {
    use serde::de::DeserializeSeed;

    let mut vals = Vec::new();
    // Split into values first so a literal glued to junk (`1\u{1}2`) is one
    // invalid token, as in jq, instead of a valid `1` followed by an error.
    for (start, end) in super::input::value_spans(input) {
        let too_deep = std::cell::Cell::new(None);
        let mut de = serde_json::Deserializer::from_str(&input[start..end]);
        let seed = JqSeed {
            depth: 0,
            max: MAX_JQ_JSON_DEPTH,
            too_deep: &too_deep,
        };
        match seed.deserialize(&mut de).and_then(|v| de.end().map(|()| v)) {
            Ok(v) => vals.push(v),
            Err(e) => {
                let msg = too_deep
                    .take()
                    .unwrap_or_else(|| format!("jq: invalid JSON: {e}"));
                return (vals, Some(msg));
            }
        }
    }
    (vals, None)
}

/// `--stream`: the events jq emits for one input value, `[path, leaf]` for
/// each scalar or empty container and `[path]` when a container closes
/// (after its last child).
pub(super) fn stream_events(v: &JqJson) -> Vec<JqJson> {
    fn walk(v: &JqJson, path: &mut Vec<JqJson>, out: &mut Vec<JqJson>) {
        let children: Vec<(JqJson, &JqJson)> = match v {
            JqJson::Array(a) => a
                .iter()
                .enumerate()
                .map(|(i, x)| (JqJson::Number(i.to_string()), x))
                .collect(),
            JqJson::Object(o) => o
                .iter()
                .map(|(k, x)| (JqJson::String(k.clone()), x))
                .collect(),
            _ => Vec::new(),
        };
        if children.is_empty() {
            out.push(JqJson::Array(vec![JqJson::Array(path.clone()), v.clone()]));
            return;
        }
        let mut last = None;
        for (k, x) in children {
            path.push(k);
            walk(x, path, out);
            last = path.pop();
        }
        if let Some(k) = last {
            let mut closing = path.clone();
            closing.push(k);
            out.push(JqJson::Array(vec![JqJson::Array(closing)]));
        }
    }
    let mut out = Vec::new();
    walk(v, &mut Vec::new(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(s: &str) -> JqJson {
        parse_json_value(s).unwrap()
    }

    #[test]
    fn round_trip_preserves_float_zero_decimal() {
        // 1.0 must NOT collapse to 1 — real jq preserves it.
        match one("1.0") {
            JqJson::Number(s) => assert_eq!(s, "1.0"),
            _ => panic!("expected Number"),
        }
    }

    #[test]
    fn integer_stays_integer() {
        match one("42") {
            JqJson::Number(s) => assert_eq!(s, "42"),
            _ => panic!("expected Number"),
        }
    }

    #[test]
    fn object_keeps_input_key_order() {
        let JqJson::Object(map) = one(r#"{"b":1,"a":2,"c":3}"#) else {
            panic!("expected Object");
        };
        let keys: Vec<&str> = map.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["b", "a", "c"]);
    }

    #[test]
    fn duplicate_key_keeps_first_position_last_value() {
        let JqJson::Object(map) = one(r#"{"a":1,"b":2,"a":3}"#) else {
            panic!("expected Object");
        };
        assert_eq!(map.len(), 2);
        assert_eq!(map[0].0, "a");
        assert!(matches!(&map[0].1, JqJson::Number(n) if n == "3"));
    }

    #[test]
    fn value_depth_limit() {
        let ok = format!("{}1{}", "[".repeat(5), "]".repeat(5));
        assert!(parse_json_value(&ok).is_ok());
        let deep = format!(
            "{}1{}",
            "[".repeat(MAX_JQ_JSON_DEPTH + 1),
            "]".repeat(MAX_JQ_JSON_DEPTH + 1)
        );
        assert!(matches!(
            parse_json_value(&deep),
            Err(JsonParseError::TooDeep(_))
        ));
    }

    #[test]
    fn value_rejects_trailing_garbage() {
        assert!(matches!(
            parse_json_value("1 2"),
            Err(JsonParseError::Invalid(_))
        ));
    }

    #[test]
    fn stream_depth_error_is_verbatim() {
        let deep = format!(
            "{}1{}",
            "[".repeat(MAX_JQ_JSON_DEPTH + 1),
            "]".repeat(MAX_JQ_JSON_DEPTH + 1)
        );
        let err = parse_json_stream(&deep).unwrap_err();
        assert!(err.starts_with("jq: JSON nesting too deep"), "{err}");
    }

    #[test]
    fn stream_reports_syntax_error() {
        let err = parse_json_stream("1 {").unwrap_err();
        assert!(err.starts_with("jq: invalid JSON:"), "{err}");
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
