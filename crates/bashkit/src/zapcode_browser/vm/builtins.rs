use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::zapcode_browser::error::{Result, ZapcodeError};
use crate::zapcode_browser::value::Value;

/// Register built-in global objects and functions.
pub fn register_globals(globals: &mut HashMap<String, Value>) {
    // Register known globals as empty objects — method calls are intercepted by the VM
    globals.insert("console".to_string(), Value::Object(IndexMap::new()));
    globals.insert("JSON".to_string(), Value::Object(IndexMap::new()));
    globals.insert("Object".to_string(), Value::Object(IndexMap::new()));
    globals.insert("Array".to_string(), Value::Object(IndexMap::new()));
    globals.insert("Promise".to_string(), Value::Object(IndexMap::new()));

    // Math gets its constants as real properties
    let mut math = IndexMap::new();
    math.insert(Arc::from("PI"), Value::Float(std::f64::consts::PI));
    math.insert(Arc::from("E"), Value::Float(std::f64::consts::E));
    math.insert(Arc::from("LN2"), Value::Float(std::f64::consts::LN_2));
    math.insert(Arc::from("LN10"), Value::Float(std::f64::consts::LN_10));
    math.insert(Arc::from("LOG2E"), Value::Float(std::f64::consts::LOG2_E));
    math.insert(Arc::from("LOG10E"), Value::Float(std::f64::consts::LOG10_E));
    math.insert(Arc::from("SQRT2"), Value::Float(std::f64::consts::SQRT_2));
    math.insert(
        Arc::from("SQRT1_2"),
        Value::Float(1.0 / std::f64::consts::SQRT_2),
    );
    globals.insert("Math".to_string(), Value::Object(math));
}

/// Execute a built-in method call. Returns Some(value) if handled, None if not a builtin.
pub fn call_builtin(
    object: &Value,
    method: &str,
    args: &[Value],
    _stdout: &mut String,
) -> Result<Option<Value>> {
    match object {
        Value::String(s) => call_string_method(s, method, args),
        Value::Array(arr) => call_array_method(arr, method, args),
        _ => Ok(None),
    }
}

/// Execute a global builtin function/method like console.log, Math.floor, JSON.parse.
pub fn call_global_method(
    global_name: &str,
    method: &str,
    args: &[Value],
    stdout: &mut String,
) -> Result<Option<Value>> {
    match global_name {
        "console" => call_console_method(method, args, stdout),
        "Math" => call_math_method(method, args),
        "JSON" => call_json_method(method, args),
        "Object" => call_object_method(method, args),
        "Array" => call_array_static_method(method, args),
        "Promise" => call_promise_method(method, args),
        _ => Ok(None),
    }
}

// ── Console ──────────────────────────────────────────────────────────

fn call_console_method(method: &str, args: &[Value], stdout: &mut String) -> Result<Option<Value>> {
    match method {
        "log" | "info" | "warn" | "error" | "debug" => {
            let output: Vec<String> = args.iter().map(|v| v.to_js_string()).collect();
            let line = output.join(" ");
            stdout.push_str(&line);
            stdout.push('\n');
            Ok(Some(Value::Undefined))
        }
        _ => Ok(None),
    }
}

// ── Math ─────────────────────────────────────────────────────────────

fn call_math_method(method: &str, args: &[Value]) -> Result<Option<Value>> {
    let result = match method {
        "abs" => {
            let n = arg_num(args, 0);
            Value::Float(n.abs())
        }
        "floor" => {
            let n = arg_num(args, 0);
            Value::Float(n.floor())
        }
        "ceil" => {
            let n = arg_num(args, 0);
            Value::Float(n.ceil())
        }
        "round" => {
            let n = arg_num(args, 0);
            Value::Float(n.round())
        }
        "trunc" => {
            let n = arg_num(args, 0);
            Value::Float(n.trunc())
        }
        "sqrt" => {
            let n = arg_num(args, 0);
            Value::Float(n.sqrt())
        }
        "cbrt" => {
            let n = arg_num(args, 0);
            Value::Float(n.cbrt())
        }
        "pow" => {
            let base = arg_num(args, 0);
            let exp = arg_num(args, 1);
            Value::Float(base.powf(exp))
        }
        "log" => {
            let n = arg_num(args, 0);
            Value::Float(n.ln())
        }
        "log2" => {
            let n = arg_num(args, 0);
            Value::Float(n.log2())
        }
        "log10" => {
            let n = arg_num(args, 0);
            Value::Float(n.log10())
        }
        "exp" => {
            let n = arg_num(args, 0);
            Value::Float(n.exp())
        }
        "sin" => {
            let n = arg_num(args, 0);
            Value::Float(n.sin())
        }
        "cos" => {
            let n = arg_num(args, 0);
            Value::Float(n.cos())
        }
        "tan" => {
            let n = arg_num(args, 0);
            Value::Float(n.tan())
        }
        "asin" => {
            let n = arg_num(args, 0);
            Value::Float(n.asin())
        }
        "acos" => {
            let n = arg_num(args, 0);
            Value::Float(n.acos())
        }
        "atan" => {
            let n = arg_num(args, 0);
            Value::Float(n.atan())
        }
        "atan2" => {
            let y = arg_num(args, 0);
            let x = arg_num(args, 1);
            Value::Float(y.atan2(x))
        }
        "max" => {
            if args.is_empty() {
                Value::Float(f64::NEG_INFINITY)
            } else {
                let mut max = arg_num(args, 0);
                for arg in &args[1..] {
                    let n = arg.to_number();
                    if n > max {
                        max = n;
                    }
                }
                Value::Float(max)
            }
        }
        "min" => {
            if args.is_empty() {
                Value::Float(f64::INFINITY)
            } else {
                let mut min = arg_num(args, 0);
                for arg in &args[1..] {
                    let n = arg.to_number();
                    if n < min {
                        min = n;
                    }
                }
                Value::Float(min)
            }
        }
        "sign" => {
            let n = arg_num(args, 0);
            if n > 0.0 {
                Value::Float(1.0)
            } else if n < 0.0 {
                Value::Float(-1.0)
            } else {
                Value::Float(0.0)
            }
        }
        "random" => {
            // Deterministic for sandbox reproducibility — use a simple LCG
            // In production this should be configurable
            Value::Float(0.5) // TODO: proper PRNG
        }
        "PI" => Value::Float(std::f64::consts::PI),
        "E" => Value::Float(std::f64::consts::E),
        _ => return Ok(None),
    };
    Ok(Some(result))
}

// ── JSON ─────────────────────────────────────────────────────────────

fn call_json_method(method: &str, args: &[Value]) -> Result<Option<Value>> {
    match method {
        "stringify" => {
            let val = args.first().unwrap_or(&Value::Undefined);
            let json = value_to_json(val);
            Ok(Some(Value::String(Arc::from(json.as_str()))))
        }
        "parse" => {
            let s = match args.first() {
                Some(Value::String(s)) => s.to_string(),
                _ => {
                    return Err(ZapcodeError::TypeError(
                        "JSON.parse requires a string argument".to_string(),
                    ))
                }
            };
            let val = json_to_value(&s)?;
            Ok(Some(val))
        }
        _ => Ok(None),
    }
}

fn value_to_json(val: &Value) -> String {
    match val {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(n) => {
            if n.is_nan() || n.is_infinite() {
                "null".to_string()
            } else {
                n.to_string()
            }
        }
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_json).collect();
            format!("[{}]", items.join(","))
        }
        Value::Object(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, value_to_json(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        Value::Function(_) | Value::BuiltinMethod { .. } | Value::Generator(_) => {
            "undefined".to_string()
        }
    }
}

/// Maximum nesting depth for JSON parsing to prevent stack overflow.
const JSON_MAX_DEPTH: usize = 64;

fn json_to_value(s: &str) -> Result<Value> {
    json_to_value_depth(s, 0)
}

fn json_to_value_depth(s: &str, depth: usize) -> Result<Value> {
    if depth > JSON_MAX_DEPTH {
        return Err(ZapcodeError::RuntimeError(
            "JSON nesting depth exceeded (max 64)".to_string(),
        ));
    }
    let s = s.trim();
    if s == "null" {
        return Ok(Value::Null);
    }
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        let unescaped = inner
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
            .replace("\\n", "\n")
            .replace("\\t", "\t");
        return Ok(Value::String(Arc::from(unescaped.as_str())));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(Value::Int(n));
    }
    if let Ok(n) = s.parse::<f64>() {
        return Ok(Value::Float(n));
    }
    if s.starts_with('[') {
        return parse_json_array(s, depth);
    }
    if s.starts_with('{') {
        return parse_json_object(s, depth);
    }
    Err(ZapcodeError::RuntimeError(format!("Invalid JSON: {}", s)))
}

fn parse_json_array(s: &str, depth: usize) -> Result<Value> {
    let inner = &s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let mut items = Vec::new();
    for part in split_json_top_level(inner) {
        items.push(json_to_value_depth(part.trim(), depth + 1)?);
    }
    Ok(Value::Array(items))
}

fn parse_json_object(s: &str, depth: usize) -> Result<Value> {
    let inner = &s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Value::Object(IndexMap::new()));
    }
    let mut map = IndexMap::new();
    for part in split_json_top_level(inner) {
        let part = part.trim();
        if let Some(colon_pos) = find_json_colon(part) {
            let key = part[..colon_pos].trim();
            let val = part[colon_pos + 1..].trim();
            let key = if key.starts_with('"') && key.ends_with('"') {
                &key[1..key.len() - 1]
            } else {
                key
            };
            map.insert(Arc::from(key), json_to_value_depth(val, depth + 1)?);
        }
    }
    Ok(Value::Object(map))
}

/// Count consecutive backslashes preceding position `i` in `bytes`.
/// A quote is escaped only if preceded by an odd number of backslashes.
fn count_preceding_backslashes(bytes: &[u8], i: usize) -> usize {
    let mut count = 0;
    let mut pos = i;
    while pos > 0 {
        pos -= 1;
        if bytes[pos] == b'\\' {
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// Returns true if the quote at position `i` is NOT escaped.
fn is_unescaped_quote(bytes: &[u8], i: usize) -> bool {
    count_preceding_backslashes(bytes, i).is_multiple_of(2)
}

fn split_json_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut start = 0;
    let bytes = s.as_bytes();

    for i in 0..bytes.len() {
        match bytes[i] {
            b'"' if !in_string => in_string = true,
            b'"' if in_string && is_unescaped_quote(bytes, i) => in_string = false,
            b'[' | b'{' if !in_string => depth += 1,
            b']' | b'}' if !in_string => depth -= 1,
            b',' if !in_string && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

fn find_json_colon(s: &str) -> Option<usize> {
    let mut in_string = false;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'"' if !in_string => in_string = true,
            b'"' if in_string && is_unescaped_quote(bytes, i) => in_string = false,
            b':' if !in_string => return Some(i),
            _ => {}
        }
    }
    None
}

// ── String methods ───────────────────────────────────────────────────

fn call_string_method(s: &Arc<str>, method: &str, args: &[Value]) -> Result<Option<Value>> {
    let result = match method {
        "length" => Value::Int(s.len() as i64),
        "charAt" => {
            let idx = arg_int(args, 0) as usize;
            match s.chars().nth(idx) {
                Some(c) => Value::String(Arc::from(c.to_string().as_str())),
                None => Value::String(Arc::from("")),
            }
        }
        "charCodeAt" => {
            let idx = arg_int(args, 0) as usize;
            match s.chars().nth(idx) {
                Some(c) => Value::Int(c as i64),
                None => Value::Float(f64::NAN),
            }
        }
        "indexOf" => {
            let search = arg_str(args, 0);
            match s.find(&*search) {
                Some(pos) => Value::Int(pos as i64),
                None => Value::Int(-1),
            }
        }
        "lastIndexOf" => {
            let search = arg_str(args, 0);
            match s.rfind(&*search) {
                Some(pos) => Value::Int(pos as i64),
                None => Value::Int(-1),
            }
        }
        "includes" => {
            let search = arg_str(args, 0);
            Value::Bool(s.contains(&*search))
        }
        "startsWith" => {
            let search = arg_str(args, 0);
            Value::Bool(s.starts_with(&*search))
        }
        "endsWith" => {
            let search = arg_str(args, 0);
            Value::Bool(s.ends_with(&*search))
        }
        "slice" => {
            let len = s.len() as i64;
            let start = normalize_index(arg_int(args, 0), len);
            let end = if args.len() > 1 {
                normalize_index(arg_int(args, 1), len)
            } else {
                len as usize
            };
            if start >= end {
                Value::String(Arc::from(""))
            } else {
                Value::String(Arc::from(&s[start..end.min(s.len())]))
            }
        }
        "substring" => {
            let len = s.len();
            let start = (arg_int(args, 0).max(0) as usize).min(len);
            let end = if args.len() > 1 {
                (arg_int(args, 1).max(0) as usize).min(len)
            } else {
                len
            };
            let (start, end) = if start > end {
                (end, start)
            } else {
                (start, end)
            };
            Value::String(Arc::from(&s[start..end]))
        }
        "toUpperCase" => Value::String(Arc::from(s.to_uppercase().as_str())),
        "toLowerCase" => Value::String(Arc::from(s.to_lowercase().as_str())),
        "trim" => Value::String(Arc::from(s.trim())),
        "trimStart" | "trimLeft" => Value::String(Arc::from(s.trim_start())),
        "trimEnd" | "trimRight" => Value::String(Arc::from(s.trim_end())),
        "repeat" => {
            let count = arg_int(args, 0).max(0) as usize;
            let result_len = s.len().saturating_mul(count);
            if result_len > 10_000_000 {
                return Err(ZapcodeError::AllocationLimitExceeded);
            }
            Value::String(Arc::from(s.repeat(count).as_str()))
        }
        "padStart" => {
            let target_len = arg_int(args, 0).max(0) as usize;
            let pad = if args.len() > 1 {
                arg_str(args, 1)
            } else {
                " ".to_string()
            };
            let current_len = s.len();
            if current_len >= target_len {
                Value::String(s.clone())
            } else {
                let pad_len = target_len - current_len;
                let padding: String = pad.chars().cycle().take(pad_len).collect();
                Value::String(Arc::from(format!("{}{}", padding, s).as_str()))
            }
        }
        "padEnd" => {
            let target_len = arg_int(args, 0).max(0) as usize;
            let pad = if args.len() > 1 {
                arg_str(args, 1)
            } else {
                " ".to_string()
            };
            let current_len = s.len();
            if current_len >= target_len {
                Value::String(s.clone())
            } else {
                let pad_len = target_len - current_len;
                let padding: String = pad.chars().cycle().take(pad_len).collect();
                Value::String(Arc::from(format!("{}{}", s, padding).as_str()))
            }
        }
        "split" => {
            let separator = arg_str(args, 0);
            let parts: Vec<Value> = if separator.is_empty() {
                s.chars()
                    .map(|c| Value::String(Arc::from(c.to_string().as_str())))
                    .collect()
            } else {
                s.split(&*separator)
                    .map(|p| Value::String(Arc::from(p)))
                    .collect()
            };
            Value::Array(parts)
        }
        "replace" => {
            let search = arg_str(args, 0);
            let replacement = arg_str(args, 1);
            Value::String(Arc::from(s.replacen(&*search, &replacement, 1).as_str()))
        }
        "replaceAll" => {
            let search = arg_str(args, 0);
            let replacement = arg_str(args, 1);
            Value::String(Arc::from(s.replace(&*search, &replacement).as_str()))
        }
        "concat" => {
            let mut result = s.to_string();
            for arg in args {
                result.push_str(&arg.to_js_string());
            }
            Value::String(Arc::from(result.as_str()))
        }
        "at" => {
            let idx = arg_int(args, 0);
            let len = s.len() as i64;
            let normalized = if idx < 0 {
                (len + idx).max(0) as usize
            } else {
                idx as usize
            };
            match s.chars().nth(normalized) {
                Some(c) => Value::String(Arc::from(c.to_string().as_str())),
                None => Value::Undefined,
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

// ── Array methods ────────────────────────────────────────────────────

fn call_array_method(arr: &[Value], method: &str, args: &[Value]) -> Result<Option<Value>> {
    let result = match method {
        "length" => Value::Int(arr.len() as i64),
        "indexOf" => {
            let search = args.first().unwrap_or(&Value::Undefined);
            let pos = arr.iter().position(|v| v.strict_eq(search));
            Value::Int(pos.map(|p| p as i64).unwrap_or(-1))
        }
        "lastIndexOf" => {
            let search = args.first().unwrap_or(&Value::Undefined);
            let pos = arr.iter().rposition(|v| v.strict_eq(search));
            Value::Int(pos.map(|p| p as i64).unwrap_or(-1))
        }
        "includes" => {
            let search = args.first().unwrap_or(&Value::Undefined);
            Value::Bool(arr.iter().any(|v| v.strict_eq(search)))
        }
        "join" => {
            let sep = if args.is_empty() {
                ",".to_string()
            } else {
                arg_str(args, 0)
            };
            let joined: Vec<String> = arr.iter().map(|v| v.to_js_string()).collect();
            Value::String(Arc::from(joined.join(&sep).as_str()))
        }
        "slice" => {
            let len = arr.len() as i64;
            let start = normalize_index(arg_int(args, 0), len);
            let end = if args.len() > 1 {
                normalize_index(arg_int(args, 1), len)
            } else {
                len as usize
            };
            if start >= end {
                Value::Array(Vec::new())
            } else {
                Value::Array(arr[start..end.min(arr.len())].to_vec())
            }
        }
        "concat" => {
            let mut result = arr.to_vec();
            for arg in args {
                match arg {
                    Value::Array(other) => result.extend_from_slice(other),
                    other => result.push(other.clone()),
                }
            }
            Value::Array(result)
        }
        "reverse" => {
            let mut result = arr.to_vec();
            result.reverse();
            Value::Array(result)
        }
        "flat" => {
            let mut result = Vec::new();
            for item in arr {
                match item {
                    Value::Array(inner) => result.extend_from_slice(inner),
                    other => result.push(other.clone()),
                }
            }
            Value::Array(result)
        }
        "at" => {
            let idx = arg_int(args, 0);
            let len = arr.len() as i64;
            let normalized = if idx < 0 {
                (len + idx).max(0) as usize
            } else {
                idx as usize
            };
            arr.get(normalized).cloned().unwrap_or(Value::Undefined)
        }
        "fill" => {
            let fill_val = args.first().unwrap_or(&Value::Undefined);
            let len = arr.len();
            let start = if args.len() > 1 {
                normalize_index(arg_int(args, 1), len as i64)
            } else {
                0
            };
            let end = if args.len() > 2 {
                normalize_index(arg_int(args, 2), len as i64)
            } else {
                len
            };
            let mut result = arr.to_vec();
            for item in result.iter_mut().take(end.min(len)).skip(start) {
                *item = fill_val.clone();
            }
            Value::Array(result)
        }
        "push" => {
            let new_len = (arr.len() + args.len()) as i64;
            Value::Int(new_len)
        }
        "pop" => arr.last().cloned().unwrap_or(Value::Undefined),
        "shift" => arr.first().cloned().unwrap_or(Value::Undefined),
        "unshift" => {
            let new_len = (arr.len() + args.len()) as i64;
            Value::Int(new_len)
        }
        "splice" => {
            let len = arr.len() as i64;
            let raw_start = if args.is_empty() { 0 } else { arg_int(args, 0) };
            let start = if raw_start < 0 {
                (len + raw_start).max(0) as usize
            } else {
                (raw_start as usize).min(arr.len())
            };
            let delete_count = if args.len() > 1 {
                (arg_int(args, 1).max(0) as usize).min(arr.len() - start)
            } else {
                arr.len() - start
            };
            let deleted: Vec<Value> = arr[start..start + delete_count].to_vec();
            Value::Array(deleted)
        }
        "every" | "some" | "map" | "filter" | "reduce" | "forEach" | "find" | "findIndex"
        | "sort" | "flatMap" => {
            // These require function callbacks — handled in VM dispatch
            return Ok(None);
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

// ── Object static methods ────────────────────────────────────────────

fn call_object_method(method: &str, args: &[Value]) -> Result<Option<Value>> {
    match method {
        "keys" => {
            let obj = args.first().unwrap_or(&Value::Undefined);
            match obj {
                Value::Object(map) => {
                    let keys: Vec<Value> = map.keys().map(|k| Value::String(k.clone())).collect();
                    Ok(Some(Value::Array(keys)))
                }
                _ => Ok(Some(Value::Array(Vec::new()))),
            }
        }
        "values" => {
            let obj = args.first().unwrap_or(&Value::Undefined);
            match obj {
                Value::Object(map) => {
                    let values: Vec<Value> = map.values().cloned().collect();
                    Ok(Some(Value::Array(values)))
                }
                _ => Ok(Some(Value::Array(Vec::new()))),
            }
        }
        "entries" => {
            let obj = args.first().unwrap_or(&Value::Undefined);
            match obj {
                Value::Object(map) => {
                    let entries: Vec<Value> = map
                        .iter()
                        .map(|(k, v)| Value::Array(vec![Value::String(k.clone()), v.clone()]))
                        .collect();
                    Ok(Some(Value::Array(entries)))
                }
                _ => Ok(Some(Value::Array(Vec::new()))),
            }
        }
        "assign" => {
            let mut target = match args.first() {
                Some(Value::Object(map)) => map.clone(),
                _ => IndexMap::new(),
            };
            for src in args.iter().skip(1) {
                if let Value::Object(map) = src {
                    for (k, v) in map {
                        target.insert(k.clone(), v.clone());
                    }
                }
            }
            Ok(Some(Value::Object(target)))
        }
        "freeze" | "seal" => {
            // No-op in sandbox — return object as-is
            Ok(args.first().cloned())
        }
        _ => Ok(None),
    }
}

fn call_array_static_method(method: &str, args: &[Value]) -> Result<Option<Value>> {
    match method {
        "isArray" => {
            let val = args.first().unwrap_or(&Value::Undefined);
            Ok(Some(Value::Bool(matches!(val, Value::Array(_)))))
        }
        "from" => {
            let val = args.first().unwrap_or(&Value::Undefined);
            match val {
                Value::Array(arr) => Ok(Some(Value::Array(arr.clone()))),
                Value::String(s) => {
                    let chars: Vec<Value> = s
                        .chars()
                        .map(|c| Value::String(Arc::from(c.to_string().as_str())))
                        .collect();
                    Ok(Some(Value::Array(chars)))
                }
                _ => Ok(Some(Value::Array(Vec::new()))),
            }
        }
        "of" => Ok(Some(Value::Array(args.to_vec()))),
        _ => Ok(None),
    }
}

// ── Promise ──────────────────────────────────────────────────────────

fn call_promise_method(method: &str, args: &[Value]) -> Result<Option<Value>> {
    match method {
        "resolve" => {
            let val = args.first().cloned().unwrap_or(Value::Undefined);
            // If the value is already a promise, return it as-is
            if is_promise(&val) {
                return Ok(Some(val));
            }
            let mut obj = IndexMap::new();
            obj.insert(Arc::from("__promise__"), Value::Bool(true));
            obj.insert(Arc::from("status"), Value::String(Arc::from("resolved")));
            obj.insert(Arc::from("value"), val);
            Ok(Some(Value::Object(obj)))
        }
        "reject" => {
            let reason = args.first().cloned().unwrap_or(Value::Undefined);
            let mut obj = IndexMap::new();
            obj.insert(Arc::from("__promise__"), Value::Bool(true));
            obj.insert(Arc::from("status"), Value::String(Arc::from("rejected")));
            obj.insert(Arc::from("reason"), reason);
            Ok(Some(Value::Object(obj)))
        }
        "all" => {
            // Basic Promise.all: takes an array of resolved promises and returns
            // a resolved promise with an array of their values.
            let arr = match args.first() {
                Some(Value::Array(arr)) => arr.clone(),
                _ => Vec::new(),
            };
            let mut results = Vec::with_capacity(arr.len());
            for item in &arr {
                if is_promise(item) {
                    if let Value::Object(map) = item {
                        if let Some(Value::String(status)) = map.get("status") {
                            if status.as_ref() == "rejected" {
                                // Promise.all rejects with the first rejection reason
                                return Ok(Some(item.clone()));
                            }
                        }
                        results.push(map.get("value").cloned().unwrap_or(Value::Undefined));
                    }
                } else {
                    results.push(item.clone());
                }
            }
            let mut obj = IndexMap::new();
            obj.insert(Arc::from("__promise__"), Value::Bool(true));
            obj.insert(Arc::from("status"), Value::String(Arc::from("resolved")));
            obj.insert(Arc::from("value"), Value::Array(results));
            Ok(Some(Value::Object(obj)))
        }
        _ => Ok(None),
    }
}

/// Check if a value is a promise object (has __promise__: true).
pub fn is_promise(val: &Value) -> bool {
    if let Value::Object(map) = val {
        matches!(map.get("__promise__"), Some(Value::Bool(true)))
    } else {
        false
    }
}

/// Create a resolved promise wrapping the given value.
pub fn make_resolved_promise(val: Value) -> Value {
    // If the value is already a promise, return it as-is (thenable unwrapping)
    if is_promise(&val) {
        return val;
    }
    let mut obj = IndexMap::new();
    obj.insert(Arc::from("__promise__"), Value::Bool(true));
    obj.insert(Arc::from("status"), Value::String(Arc::from("resolved")));
    obj.insert(Arc::from("value"), val);
    Value::Object(obj)
}

// ── Helpers ──────────────────────────────────────────────────────────

fn arg_num(args: &[Value], idx: usize) -> f64 {
    args.get(idx).map(|v| v.to_number()).unwrap_or(f64::NAN)
}

fn arg_int(args: &[Value], idx: usize) -> i64 {
    args.get(idx)
        .map(|v| match v {
            Value::Int(n) => *n,
            other => other.to_number() as i64,
        })
        .unwrap_or(0)
}

fn arg_str(args: &[Value], idx: usize) -> String {
    args.get(idx).map(|v| v.to_js_string()).unwrap_or_default()
}

fn normalize_index(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}
