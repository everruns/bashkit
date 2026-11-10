use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Arc<str>),
    Array(Vec<Value>),
    Object(IndexMap<Arc<str>, Value>),
    Function(Closure),
    /// A generator object — calling function* creates one of these.
    /// Generators are stateful and cannot be serialized mid-yield.
    #[serde(skip)]
    Generator(GeneratorObject),
    /// Internal: a bound method on a built-in object (e.g., console.log, Math.floor).
    /// Not visible to user code — used to dispatch builtin calls.
    #[serde(skip)]
    BuiltinMethod {
        object_name: Arc<str>,
        method_name: Arc<str>,
    },
}

/// Identifies a function in the compiled program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionId(pub usize);

/// A closure captures the enclosing scope's variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Closure {
    pub func_id: FunctionId,
    pub captured: Vec<(String, Value)>,
}

/// The state of a generator object.
#[derive(Debug, Clone)]
pub struct GeneratorObject {
    /// Unique ID for this generator instance (used as key in VM generator registry).
    pub id: u64,
    /// The function this generator was created from.
    pub func_id: FunctionId,
    /// Captured closure variables.
    pub captured: Vec<(String, Value)>,
    /// Suspended execution state. None = not yet started.
    pub suspended: Option<SuspendedFrame>,
    /// Whether the generator has completed.
    pub done: bool,
}

/// Saved execution state of a suspended generator.
#[derive(Debug, Clone)]
pub struct SuspendedFrame {
    pub ip: usize,
    pub locals: Vec<Value>,
    pub stack: Vec<Value>,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::Float(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "object",
            Value::Object(_) => "object",
            Value::Function(_) | Value::BuiltinMethod { .. } => "function",
            Value::Generator(_) => "object",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(_)
            | Value::Object(_)
            | Value::Function(_)
            | Value::BuiltinMethod { .. }
            | Value::Generator(_) => true,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(true) => 1.0,
            Value::Bool(false) => 0.0,
            Value::Int(n) => *n as f64,
            Value::Float(n) => *n,
            Value::String(s) => s.parse::<f64>().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }

    pub fn to_js_string(&self) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(n) => {
                if n.is_infinite() {
                    if *n > 0.0 {
                        "Infinity".to_string()
                    } else {
                        "-Infinity".to_string()
                    }
                } else if n.is_nan() {
                    "NaN".to_string()
                } else {
                    // Remove trailing ".0" for whole numbers
                    n.to_string()
                }
            }
            Value::String(s) => s.to_string(),
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_js_string()).collect();
                items.join(",")
            }
            Value::Object(_) => "[object Object]".to_string(),
            Value::Function(_) | Value::BuiltinMethod { .. } => "function".to_string(),
            Value::Generator(_) => "[object Generator]".to_string(),
        }
    }

    /// Strict equality (===)
    pub fn strict_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::String(a), Value::String(b)) => a == b,
            // Reference equality for arrays/objects
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_js_string())
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.strict_eq(other)
    }
}
