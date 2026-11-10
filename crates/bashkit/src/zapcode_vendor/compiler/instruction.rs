use serde::{Deserialize, Serialize};

/// Bytecode instructions for the Zapcode VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    // Stack
    Push(Constant),
    Pop,
    Dup,

    // Variables
    LoadLocal(usize),
    StoreLocal(usize),
    LoadGlobal(String),
    StoreGlobal(String),
    DeclareLocal(String),

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Neg,
    BitNot,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,

    // Comparison
    Eq,
    Neq,
    StrictEq,
    StrictNeq,
    Lt,
    Lte,
    Gt,
    Gte,

    // Logical
    Not,

    // Objects & Arrays
    CreateArray(usize),
    CreateObject(usize),
    GetProperty(String),
    SetProperty(String),
    GetIndex,
    SetIndex,
    Spread,
    In,
    InstanceOf,

    // Functions
    CreateClosure(usize),
    Call(usize),
    Return,
    CallExternal(String, usize),

    // Control flow
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    JumpIfNullish(usize),

    // Loops
    SetupLoop,
    Break,
    Continue,

    // Iterators
    GetIterator,
    IteratorNext,
    IteratorDone,

    // Error handling
    SetupTry(usize, Option<usize>),
    Throw,
    EndTry,

    // Typeof
    TypeOf,

    // Void
    Void,

    // Update
    Increment,
    Decrement,

    // Template literals
    ConcatStrings(usize),

    // Destructuring
    DestructureObject(Vec<String>),
    DestructureArray(usize),

    // Classes
    /// Create a class: pops constructor closure (or undefined), then n_methods method closures
    /// with method names, then n_static static closures with names, then optional super class.
    /// Pushes the class object (an Object with __constructor__, __prototype__, __class_name__).
    CreateClass {
        name: String,
        n_methods: usize,
        n_statics: usize,
        has_super: bool,
    },
    /// Construct: pops class object + args, creates instance, calls constructor, pushes instance.
    Construct(usize),
    /// Load `this` from the current call frame.
    LoadThis,
    /// Store a value as the current `this` (used for this.prop = val).
    StoreThis,
    /// Call super constructor with n args. Pops args, looks up __super__.__constructor__,
    /// calls it with current `this`.
    CallSuper(usize),

    // Generators
    /// Create a generator object from a function index (like CreateClosure but for generators).
    CreateGenerator(usize),
    /// Yield a value from a generator. Pops the value, suspends execution.
    Yield,

    /// Await: if the top-of-stack is a resolved Promise object, unwrap its value.
    /// If it's a regular value, leave it as-is. External call suspension is handled
    /// by CallExternal before Await is reached.
    Await,

    // Misc
    Nop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Constant {
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}
