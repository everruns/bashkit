//! Bounded JSON values at foreign host-function boundaries.
//!
//! Native Rust handlers retain their language-specific value APIs. Bindings
//! using JSON share this explicit subset instead of stringifying unsupported
//! objects or serializing VM implementation details. Hosts own I/O and service
//! authorization; conversion does not grant further sandbox capabilities.

use serde_json::Value;

use crate::{Error, Result};

/// Maximum nesting across a foreign host-function boundary.
pub const MAX_DEPTH: usize = 64;

fn depth_guard(depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::Execution(
            "host value nesting depth exceeds maximum of 64".into(),
        ));
    }
    Ok(())
}

/// Validate the shared JSON subset (including signed 64-bit integer bounds).
pub fn validate(value: &Value) -> Result<()> {
    fn walk(value: &Value, depth: usize) -> Result<()> {
        depth_guard(depth)?;
        match value {
            Value::Number(n) if n.is_u64() && n.as_i64().is_none() => Err(Error::Execution(
                "host integers must fit in signed 64 bits".into(),
            )),
            // serde_json falls back to f64 for integer literals beyond i64/u64.
            // Reject those rounded boundary values instead of changing an
            // oversized integer into a different floating-point value.
            Value::Number(n)
                if n.is_f64()
                    && n.as_f64()
                        .is_some_and(|v| v >= i64::MAX as f64 || v <= i64::MIN as f64) =>
            {
                Err(Error::Execution(
                    "host numbers exceed signed 64-bit range".into(),
                ))
            }
            Value::Array(values) => values.iter().try_for_each(|v| walk(v, depth + 1)),
            Value::Object(values) => values.values().try_for_each(|v| walk(v, depth + 1)),
            _ => Ok(()),
        }
    }
    walk(value, 0)
}

#[cfg(any(feature = "python", feature = "typescript"))]
fn number(value: f64) -> Result<Value> {
    let value = serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| Error::Execution("host numbers must be finite".into()))?;
    validate(&value)?;
    Ok(value)
}

/// Encode a Python value as JSON, rejecting unsupported objects.
#[cfg(feature = "python")]
pub fn encode_python(value: &crate::MontyObject) -> Result<Value> {
    fn encode(value: &crate::MontyObject, depth: usize) -> Result<Value> {
        use crate::MontyObject as M;
        depth_guard(depth)?;
        Ok(match value {
            M::None => Value::Null,
            M::Bool(v) => Value::Bool(*v),
            M::Int(v) => Value::from(*v),
            M::BigInt(v) => Value::from(v.to_string().parse::<i64>().map_err(|_| {
                Error::Execution("host integers must fit in signed 64 bits".into())
            })?),
            M::Float(v) => number(*v)?,
            M::String(v) => Value::String(v.clone()),
            M::List(values) | M::Tuple(values) => Value::Array(
                values
                    .iter()
                    .map(|v| encode(v, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            M::Dict(values) => {
                let mut object = serde_json::Map::new();
                for (key, value) in values {
                    let M::String(key) = key else {
                        return Err(Error::Execution(
                            "host dictionaries require string keys".into(),
                        ));
                    };
                    object.insert(key.clone(), encode(value, depth + 1)?);
                }
                Value::Object(object)
            }
            _ => {
                return Err(Error::Execution(
                    "Python host arguments must be JSON values".into(),
                ));
            }
        })
    }
    encode(value, 0)
}

/// Decode JSON into the Python host-function value subset.
#[cfg(feature = "python")]
pub fn decode_python(value: &Value) -> Result<crate::MontyObject> {
    fn decode(value: &Value) -> crate::MontyObject {
        use crate::MontyObject as M;
        match value {
            Value::Null => M::None,
            Value::Bool(v) => M::Bool(*v),
            Value::Number(v) => match v.as_i64() {
                Some(v) => M::Int(v),
                None => M::Float(v.as_f64().expect("validated JSON number")),
            },
            Value::String(v) => M::String(v.clone()),
            Value::Array(values) => M::List(values.iter().map(decode).collect()),
            Value::Object(values) => M::dict(
                values
                    .iter()
                    .map(|(k, v)| (M::String(k.clone()), decode(v)))
                    .collect::<Vec<_>>(),
            ),
        }
    }
    validate(value)?;
    Ok(decode(value))
}

/// Encode a TypeScript value as JSON; undefined maps to null.
#[cfg(feature = "typescript")]
pub fn encode_typescript(value: &crate::ZapcodeValue) -> Result<Value> {
    fn encode(value: &crate::ZapcodeValue, depth: usize) -> Result<Value> {
        use crate::ZapcodeValue as Z;
        depth_guard(depth)?;
        Ok(match value {
            Z::Undefined | Z::Null => Value::Null,
            Z::Bool(v) => Value::Bool(*v),
            Z::Int(v) => Value::from(*v),
            Z::Float(v) => number(*v)?,
            Z::String(v) => Value::String(v.to_string()),
            Z::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|v| encode(v, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Z::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(k, v)| Ok((k.to_string(), encode(v, depth + 1)?)))
                    .collect::<Result<_>>()?,
            ),
            _ => {
                return Err(Error::Execution(
                    "TypeScript host arguments must be JSON values".into(),
                ));
            }
        })
    }
    encode(value, 0)
}

/// Decode JSON into the TypeScript host-function value subset.
#[cfg(feature = "typescript")]
pub fn decode_typescript(value: &Value) -> Result<crate::ZapcodeValue> {
    fn decode(value: &Value) -> crate::ZapcodeValue {
        use crate::ZapcodeValue as Z;
        use std::sync::Arc;
        match value {
            Value::Null => Z::Null,
            Value::Bool(v) => Z::Bool(*v),
            Value::Number(v) => match v.as_i64() {
                Some(v) => Z::Int(v),
                None => Z::Float(v.as_f64().expect("validated JSON number")),
            },
            Value::String(v) => Z::String(Arc::from(v.as_str())),
            Value::Array(values) => Z::Array(values.iter().map(decode).collect()),
            Value::Object(values) => Z::Object(
                values
                    .iter()
                    .map(|(k, v)| (Arc::from(k.as_str()), decode(v)))
                    .collect(),
            ),
        }
    }
    validate(value)?;
    Ok(decode(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_deep_values_and_unsigned_overflow() {
        let mut nested = Value::Null;
        for _ in 0..MAX_DEPTH + 2 {
            nested = Value::Array(vec![nested]);
        }
        assert!(validate(&nested).is_err());
        assert!(validate(&Value::from(u64::MAX)).is_err());
        assert!(validate(&Value::from(i64::MAX)).is_ok());
        assert!(validate(&Value::from(i64::MIN)).is_ok());
        for source in ["-9223372036854775809", "18446744073709551616", "1e30"] {
            assert!(validate(&serde_json::from_str::<Value>(source).unwrap()).is_err());
        }
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_roundtrip_and_unsupported_values() {
        use crate::MontyObject as M;
        let value = serde_json::json!({"items": [null, true, 42, 0.5, "text"]});
        assert_eq!(
            encode_python(&decode_python(&value).unwrap()).unwrap(),
            value
        );
        assert!(encode_python(&M::Float(f64::NAN)).is_err());
        assert!(encode_python(&M::Bytes(vec![1])).is_err());
        assert!(encode_python(&M::dict(vec![(M::Int(1), M::None)])).is_err());
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_roundtrip_and_undefined() {
        let value = serde_json::json!({"items": [null, true, 42, 0.5, "text"]});
        assert_eq!(
            encode_typescript(&decode_typescript(&value).unwrap()).unwrap(),
            value
        );
        assert_eq!(
            encode_typescript(&crate::ZapcodeValue::Undefined).unwrap(),
            Value::Null
        );
        assert!(encode_typescript(&crate::ZapcodeValue::Float(f64::INFINITY)).is_err());
    }
}
