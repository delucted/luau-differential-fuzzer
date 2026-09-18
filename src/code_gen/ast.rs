pub type Ident = String;

// ---------------------------------------------------------------------------
// Literals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Nil,
    Boolean(bool),
    /// Print with enough precision to round-trip. `-0.0`, `NaN` and `inf` have
    /// no literal syntax; emit them as `-0`, `0/0` and `math.huge` instead.
    Number(f64),
    /// Raw bytes, not yet escaped. The printer is responsible for escaping.
    String(String),
}

// ---------------------------------------------------------------------------
// Operators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
    Not,
    Len,
}

/// Operators allowed in compound assignment (`a += 1`). Comparison and
/// logical operators are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompoundOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    Concat,
}

// ---------------------------------------------------------------------------
// Type annotations (printed; may be truthful or deliberately wrong)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnot {
    Number,
    String,
    Boolean,
    Nil,
    Any,
    /// `T?`
    Optional(Box<TypeAnnot>),
    /// `{ T }`, an array of T
    Array(Box<TypeAnnot>),
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// A function call. Shared between expression position and statement
/// position, because the same call can appear in both.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub callee: Box<Expr>,
    /// `Some(name)` for `obj:name(args)`, `None` for `callee(args)`.
    pub method: Option<Ident>,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TableField {
    /// `{ 1, 2 }`
    Positional(Expr),
    /// `{ x = 1 }`
    Named { name: Ident, value: Expr },
    /// `{ [k] = v }`
    Keyed { key: Expr, value: Expr },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub annotation: Option<TypeAnnot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionBody {
    pub params: Vec<Param>,
    /// Whether the parameter list ends in `...`.
    pub is_vararg: bool,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    /// Local, upvalue or global. Which one is a property of the enclosing
    /// scope, not of the node; the generator's scope table knows.
    Var(Ident),
    /// `...`. Only valid inside a vararg function (the chunk itself is one).
    VarArgs,
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Explicit parentheses. Semantically meaningful: truncates a
    /// multi-value expression (call or `...`) to exactly one value.
    Paren(Box<Expr>),
    /// `object[key]`
    Index {
        object: Box<Expr>,
        key: Box<Expr>,
    },
    /// `object.name`
    Field {
        object: Box<Expr>,
        name: Ident,
    },
    Call(Call),
    /// Anonymous function. Captures enclosing locals as upvalues.
    Function(FunctionBody),
    Table(Vec<TableField>),
    /// `if cond then a else b`
    IfElse {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprKind {
    Literal,
    Unary,
    Binary,
    Paren,
    Function,
    Table,
    IfElse
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

/// Assignment target. Function calls cannot be assigned to, and the type
/// makes that unrepresentable.
#[derive(Debug, Clone, PartialEq)]
pub enum LValue {
    Var(Ident),
    Index { object: Expr, key: Expr },
    Field { object: Expr, name: Ident },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalName {
    pub name: Ident,
    pub annotation: Option<TypeAnnot>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `local a: number, b = 1, 2`. Fewer values than names pads with nil;
    /// more values than names discards the extras. Both are legal and both
    /// are worth generating.
    Local {
        names: Vec<LocalName>,
        values: Vec<Expr>,
    },
    /// `a, t[1] = 1, 2`. Targets are evaluated before any assignment happens.
    Assign {
        targets: Vec<LValue>,
        values: Vec<Expr>,
    },
    /// `a += 1`. Exactly one target, one value.
    CompoundAssign {
        target: LValue,
        op: CompoundOp,
        value: Expr,
    },
    /// `local function name(...) ... end`. The name is in scope inside the
    /// body, so recursion is possible; the generator must bound it.
    LocalFunction {
        name: Ident,
        body: FunctionBody,
    },
    /// `function name(...) ... end`, a global.
    Function {
        name: Ident,
        body: FunctionBody,
    },
    /// A call in statement position. Its return values are discarded.
    Call(Call),
    /// `do ... end`. Introduces a scope and nothing else.
    Do(Block),
    While {
        cond: Expr,
        body: Block,
    },
    /// `repeat ... until cond`. Unlike every other construct, `cond` is
    /// evaluated inside the body's scope and can see its locals.
    Repeat {
        body: Block,
        until: Expr,
    },
    If {
        cond: Expr,
        then: Block,
        elseifs: Vec<(Expr, Block)>,
        else_: Option<Block>,
    },
    /// `for var = start, limit[, step] do ... end`. The loop variable is a
    /// fresh local on every iteration, so closures created in the body
    /// capture per-iteration copies. Constant `start`, `limit` and `step`
    /// with a small trip count is what makes O2 unroll the loop.
    NumericFor {
        var: Ident,
        start: Expr,
        limit: Expr,
        step: Option<Expr>,
        body: Block,
    },
    /// `for k, v in iter do ... end`. Luau also allows `for k, v in t do`
    /// with no iterator function (generalised iteration). Iterating the hash
    /// part of a table has unspecified order and must never be generated;
    /// stick to `ipairs` and array-shaped tables.
    GenericFor {
        vars: Vec<Ident>,
        iter: Vec<Expr>,
        body: Block,
    },
}

/// Statements that may only appear last in a block.
#[derive(Debug, Clone, PartialEq)]
pub enum LastStmt {
    Return(Vec<Expr>),
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub last: Option<LastStmt>,
}

/// A whole generated program. The chunk is itself a vararg function, so
/// top-level `return` and `...` are legal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub body: Block,
}