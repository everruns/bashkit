//! awk syntax tree.
//!
//! Decisions:
//! - Variables are resolved at parse time to slots: `Global(i)` indexes the
//!   interpreter's global table (special variables first, see `special`),
//!   `Local(i)` a function parameter in the current call frame.
//! - Lvalues are the expression forms `Var`, `Elem` and `Field`; assignment
//!   targets are checked by the parser.
//! - `Elem` keeps one subscript group per `[...]` so gawk arrays of arrays
//!   (`a[i][j]`) work; commas inside one group join with `SUBSEP`.
//! - User function calls hold a function index; definitions may follow use,
//!   undefined functions are reported after parsing (gawk does the same).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VarRef {
    Global(u32),
    Local(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Bi {
    Length,
    Substr,
    Index,
    Split,
    Sub,
    Gsub,
    Gensub,
    Match,
    Sprintf,
    Sin,
    Cos,
    Atan2,
    Exp,
    Log,
    Sqrt,
    Int,
    Rand,
    Srand,
    Tolower,
    Toupper,
    System,
    Close,
    Fflush,
    Strtonum,
    Systime,
    Strftime,
    Mktime,
    Asort,
    Asorti,
    Typeof,
    Isarray,
    And,
    Or,
    Xor,
    Lshift,
    Rshift,
    Compl,
    Patsplit,
}

impl Bi {
    pub(super) fn from_name(name: &str) -> Option<Bi> {
        Some(match name {
            "length" => Bi::Length,
            "substr" => Bi::Substr,
            "index" => Bi::Index,
            "split" => Bi::Split,
            "sub" => Bi::Sub,
            "gsub" => Bi::Gsub,
            "gensub" => Bi::Gensub,
            "match" => Bi::Match,
            "sprintf" => Bi::Sprintf,
            "sin" => Bi::Sin,
            "cos" => Bi::Cos,
            "atan2" => Bi::Atan2,
            "exp" => Bi::Exp,
            "log" => Bi::Log,
            "sqrt" => Bi::Sqrt,
            "int" => Bi::Int,
            "rand" => Bi::Rand,
            "srand" => Bi::Srand,
            "tolower" => Bi::Tolower,
            "toupper" => Bi::Toupper,
            "system" => Bi::System,
            "close" => Bi::Close,
            "fflush" => Bi::Fflush,
            "strtonum" => Bi::Strtonum,
            "systime" => Bi::Systime,
            "strftime" => Bi::Strftime,
            "mktime" => Bi::Mktime,
            "asort" => Bi::Asort,
            "asorti" => Bi::Asorti,
            "typeof" => Bi::Typeof,
            "isarray" => Bi::Isarray,
            "and" => Bi::And,
            "or" => Bi::Or,
            "xor" => Bi::Xor,
            "lshift" => Bi::Lshift,
            "rshift" => Bi::Rshift,
            "compl" => Bi::Compl,
            "patsplit" => Bi::Patsplit,
            _ => return None,
        })
    }

    /// Argument positions that name an array (not evaluated as values).
    pub(super) fn array_args(self) -> &'static [usize] {
        match self {
            Bi::Split | Bi::Patsplit => &[1, 3],
            Bi::Match => &[2],
            Bi::Asort | Bi::Asorti => &[0, 1],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum GetlineSrc {
    /// Plain `getline`: the main input.
    Main,
    /// `getline < file`
    File(Box<Expr>),
    /// `cmd | getline`
    Cmd(Box<Expr>),
}

#[derive(Debug, Clone)]
pub(super) enum Expr {
    Num(f64),
    Str(String),
    /// A regex literal in value position: `$0 ~ /re/`.
    Regex(u32),
    Var(VarRef),
    Elem(VarRef, Vec<Vec<Expr>>),
    Field(Box<Expr>),
    /// `lv = e` or `lv op= e`.
    Assign(Box<Expr>, Option<BinOp>, Box<Expr>),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    Not(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// `a ~ b` (or `!~` when the flag is set).
    Match(bool, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Concat(Box<Expr>, Box<Expr>),
    /// `(subs) in array`
    In(Vec<Expr>, Box<Expr>),
    /// `++lv`, `lv--`, ...: (lvalue, delta, is_prefix)
    IncDec(Box<Expr>, f64, bool),
    Call(u32, Vec<Expr>),
    /// `@name(args)`: function chosen by the value of a variable.
    Indirect(Box<Expr>, Vec<Expr>),
    Builtin(Bi, Vec<Expr>),
    Getline(GetlineSrc, Option<Box<Expr>>),
    /// Parenthesized expression (kept to tell `(a, b) in x` and
    /// `print (a) > f` from other forms; evaluated transparently).
    Group(Box<Expr>),
    /// `(a, b)` grouping, only valid before `in` or as print arguments.
    List(Vec<Expr>),
}

impl Expr {
    pub(super) fn is_lvalue(&self) -> bool {
        matches!(self, Expr::Var(_) | Expr::Elem(..) | Expr::Field(_))
    }
}

#[derive(Debug, Clone)]
pub(super) enum Redirect {
    File(Expr),
    Append(Expr),
    Pipe(Expr),
}

#[derive(Debug, Clone)]
pub(super) struct Stmt {
    pub(super) kind: StmtKind,
    /// Index into `Program::locs`.
    pub(super) loc: u32,
}

pub(super) type Block = Vec<Stmt>;

#[derive(Debug, Clone)]
pub(super) enum StmtKind {
    Expr(Expr),
    Print(Vec<Expr>, Option<Redirect>),
    Printf(Vec<Expr>, Option<Redirect>),
    If(Expr, Block, Option<Block>),
    While(Expr, Block),
    Do(Block, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Box<Stmt>>, Block),
    /// `for (lv in array)`
    ForIn(Expr, Expr, Block),
    Block(Block),
    Next,
    NextFile,
    Exit(Option<Expr>),
    Return(Option<Expr>),
    Break,
    Continue,
    /// `delete array` or `delete array[subs]` (the expression is the array
    /// or element form).
    Delete(Expr),
    Switch(Expr, Vec<(CaseLabel, Block)>),
}

#[derive(Debug, Clone)]
pub(super) enum CaseLabel {
    Value(Expr),
    Regex(u32),
    Default,
}

#[derive(Debug, Clone)]
pub(super) enum Pattern {
    All,
    Expr(Expr),
    /// Range pattern; the index is its slot in the range-state table.
    Range(Expr, Expr, usize),
}

#[derive(Debug, Clone)]
pub(super) struct Rule {
    pub(super) pattern: Pattern,
    /// `None`: the default action, `print $0`.
    pub(super) body: Option<Block>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Func {
    pub(super) name: String,
    pub(super) params: Vec<String>,
    pub(super) body: Block,
    /// Per parameter: used as an array inside the body (directly or by
    /// passing it on). An untyped argument for such a parameter becomes an
    /// array in the caller.
    pub(super) array_params: Vec<bool>,
    pub(super) defined: bool,
}

/// Source text of one program piece (`-f file`, `-e text`, the command line).
#[derive(Debug, Clone)]
pub(super) struct Source {
    /// Label in messages: `cmd. line` or the file name.
    pub(super) name: String,
    /// Byte offset of this source in the joined program text.
    pub(super) start: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Program {
    pub(super) begin: Vec<Block>,
    pub(super) end: Vec<Block>,
    pub(super) beginfile: Vec<Block>,
    pub(super) endfile: Vec<Block>,
    pub(super) rules: Vec<Rule>,
    pub(super) funcs: Vec<Func>,
    /// Global variable names; index = `VarRef::Global` slot.
    pub(super) globals: Vec<String>,
    /// Regex literal sources; index = `Expr::Regex` id.
    pub(super) regexes: Vec<String>,
    /// Statement locations: (source label, line number).
    pub(super) locs: Vec<(String, u32)>,
    pub(super) ranges: usize,
}

/// Special variables, in global slot order.
pub(super) mod special {
    pub(crate) const NAMES: &[&str] = &[
        "NR",
        "NF",
        "FNR",
        "FS",
        "OFS",
        "ORS",
        "RS",
        "FILENAME",
        "SUBSEP",
        "RSTART",
        "RLENGTH",
        "CONVFMT",
        "OFMT",
        "ENVIRON",
        "ARGC",
        "ARGV",
        "RT",
        "IGNORECASE",
        "FIELDWIDTHS",
        "FPAT",
        "PROCINFO",
        "ERRNO",
        "SYMTAB",
    ];
    pub(crate) const NR: u32 = 0;
    pub(crate) const NF: u32 = 1;
    pub(crate) const FNR: u32 = 2;
    pub(crate) const FS: u32 = 3;
    pub(crate) const OFS: u32 = 4;
    pub(crate) const ORS: u32 = 5;
    pub(crate) const RS: u32 = 6;
    pub(crate) const FILENAME: u32 = 7;
    pub(crate) const SUBSEP: u32 = 8;
    pub(crate) const RSTART: u32 = 9;
    pub(crate) const RLENGTH: u32 = 10;
    pub(crate) const CONVFMT: u32 = 11;
    pub(crate) const OFMT: u32 = 12;
    pub(crate) const ENVIRON: u32 = 13;
    pub(crate) const ARGC: u32 = 14;
    pub(crate) const ARGV: u32 = 15;
    pub(crate) const RT: u32 = 16;
    pub(crate) const IGNORECASE: u32 = 17;
    pub(crate) const FIELDWIDTHS: u32 = 18;
    pub(crate) const FPAT: u32 = 19;
    pub(crate) const PROCINFO: u32 = 20;
    pub(crate) const ERRNO: u32 = 21;
    pub(crate) const SYMTAB: u32 = 22;
    pub(crate) const COUNT: u32 = 23;
}
