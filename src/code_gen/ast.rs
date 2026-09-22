use std::fmt::Write;
pub type Ident = String;

pub trait Printer {
    /// `indent` is the level the *current line* started at. Anything that opens
    /// a block writes its contents at `indent + 1` and closes at `indent`.
    fn print_to(&self, out: &mut String, indent: usize);

    fn print(&self) -> String {
        let mut out = String::new();
        self.print_to(&mut out, 0);
        out
    }
}

/// One tab per level.
fn write_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push('\t');
    }
}

fn print_list<T: Printer>(items: &[T], out: &mut String, indent: usize) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        item.print_to(out, indent);
    }
}

fn print_idents(names: &[Ident], out: &mut String) {
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(name);
    }
}

// Binding power, loosest first. Lua's precedence table, plus `if .. then ..
// else ..` below all of it: Luau only takes that one unparenthesised when it
// is the whole expression.
const PREC_IFELSE: u8 = 0;
const PREC_OR: u8 = 1;
const PREC_AND: u8 = 2;
const PREC_CMP: u8 = 3;
const PREC_CONCAT: u8 = 4;
const PREC_ADD: u8 = 5;
const PREC_MUL: u8 = 6;
const PREC_UNARY: u8 = 7;
const PREC_POW: u8 = 8;
/// Literals, names, calls, tables and anything already bracketed.
const PREC_PRIMARY: u8 = 9;

/// Name of the value dumper emitted ahead of a program that prints anything.
const DUMP_FN: &str = "__dump";

/// Values per `print` call. Each argument holds a register while the call is
/// set up, and a Luau function has 255 of them for everything it does.
const PRINT_BATCH: usize = 16;

/// Printing a table or a function gives `table: 0x55f3...`, an address that
/// changes between runs and would make every program look like a divergence.
/// This walks values instead. Keys are sorted, because `pairs` order is not
/// specified and the two oracles must agree on it.
const DUMP_PRELUDE: &str = "\
local function __dump(v, d)
\td = d or 0
\tlocal t = type(v)
\tif t ~= \"table\" then
\t\tif t == \"function\" or t == \"thread\" or t == \"userdata\" then
\t\t\treturn t
\t\tend
\t\treturn tostring(v)
\tend
\tif d > 3 then
\t\treturn \"{...}\"
\tend
\tlocal keys = {}
\tfor k in pairs(v) do
\t\tkeys[#keys + 1] = k
\tend
\ttable.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
\tlocal parts = {}
\tfor i = 1, #keys do
\t\tparts[i] = \"[\" .. __dump(keys[i], d + 1) .. \"]=\" .. __dump(v[keys[i]], d + 1)
\tend
\treturn \"{\" .. table.concat(parts, \",\") .. \"}\"
end

";

fn print_number(n: f64, out: &mut String) {
    if n.is_nan() {
        out.push_str("(0/0)");
    } else if n.is_infinite() {
        out.push_str(if n > 0.0 { "(1/0)" } else { "(-1/0)" });
    } else if n == 0.0 && n.is_sign_negative() {
        out.push_str("(-0)");
    } else if n < 0.0 {
        write!(out, "({:?})", n).unwrap();
    } else {
        write!(out, "{:?}", n).unwrap();
    }
}

fn print_string(bytes: &[u8], out: &mut String) {
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => write!(out, "\\{:03}", b).unwrap(),
        }
    }
    out.push('"');
}

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

impl Printer for Literal {
    fn print_to(&self, out: &mut String, _indent: usize) {
        match self {
            Self::Nil => out.push_str("nil"),
            Self::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => print_number(*n, out),
            Self::String(bytes) => print_string(bytes.as_ref(), out),
        }
    }
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

impl BinOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::FloorDiv => "//",
            BinOp::Mod => "%",
            BinOp::Pow => "^",
            BinOp::Concat => "..",
            BinOp::Eq => "==",
            BinOp::Ne => "~=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or"
        }
    }

    fn prec(&self) -> u8 {
        match self {
            BinOp::Or => PREC_OR,
            BinOp::And => PREC_AND,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => PREC_CMP,
            BinOp::Concat => PREC_CONCAT,
            BinOp::Add | BinOp::Sub => PREC_ADD,
            BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Mod => PREC_MUL,
            BinOp::Pow => PREC_POW
        }
    }

    /// `..` and `^` are the two that group to the right.
    fn is_right_assoc(&self) -> bool {
        matches!(self, BinOp::Concat | BinOp::Pow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
    Not,
    Len,
}

impl UnOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Not => "not ",
            Self::Len => "#"
        }
    }
}

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
impl CompoundOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "+=",
            Self::Sub => "-=",
            Self::Mul => "*=",
            Self::Div => "/=",
            Self::FloorDiv => "//=",
            Self::Mod => "%=",
            Self::Pow => "^=",
            Self::Concat => "..=",
        }
    }
}

// ---------------------------------------------------------------------------
// Type annotations
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

impl Printer for TypeAnnot {
    fn print_to(&self, out: &mut String, indent: usize) {
        match self {
            Self::Number => out.push_str("number"),
            Self::String => out.push_str("string"),
            Self::Boolean => out.push_str("boolean"),
            Self::Nil => out.push_str("nil"),
            Self::Any => out.push_str("any"),
            Self::Optional(inner) => {
                inner.print_to(out, indent);
                out.push('?');
            },
            Self::Array(inner) => {
                out.push_str("{ ");
                inner.print_to(out, indent);
                out.push_str(" }");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------


#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub callee: Box<Expr>,
    /// `Some(name)` for `obj:name(args)`, `None` for `callee(args)`.
    pub method: Option<Ident>,
    pub args: Vec<Expr>,
}

impl Printer for Call {
    fn print_to(&self, out: &mut String, indent: usize) {
        self.callee.print_prefix(out, indent);

        if let Some(name) = &self.method {
            out.push(':');
            out.push_str(name);
        }

        out.push('(');
        print_list(&self.args, out, indent);
        out.push(')');
    }
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

impl Printer for TableField {
    fn print_to(&self, out: &mut String, indent: usize) {
        match self {
            Self::Positional(value) => value.print_to(out, indent),
            Self::Named { name, value } => {
                out.push_str(name);
                out.push_str(" = ");
                value.print_to(out, indent);
            },
            Self::Keyed { key, value } => {
                out.push('[');
                key.print_to(out, indent);
                out.push_str("] = ");
                value.print_to(out, indent);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub annotation: Option<TypeAnnot>,
}

impl Printer for Param {
    fn print_to(&self, out: &mut String, indent: usize) {
        out.push_str(&self.name);
        if let Some(annot) = &self.annotation {
            out.push_str(": ");
            annot.print_to(out, indent);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionBody {
    pub params: Vec<Param>,
    /// Whether the parameter list ends in `...`.
    pub is_vararg: bool,
    pub body: Block,
}

impl Printer for FunctionBody {
    /// From the parameter list on. The `function` keyword belongs to the
    /// caller, since a named function puts its name in between.
    fn print_to(&self, out: &mut String, indent: usize) {
        out.push('(');
        print_list(&self.params, out, indent);
        if self.is_vararg {
            if !self.params.is_empty() {
                out.push_str(", ");
            }
            out.push_str("...");
        }
        out.push_str(")\n");
        self.body.print_to(out, indent + 1);
        write_indent(out, indent);
        out.push_str("end");
    }
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

impl Expr {
    /// What may sit to the left of `[k]`, `.name` or a call. Anything else has
    /// to be wrapped in parentheses first: `5[1]` does not parse, `(5)[1] `does.
    pub fn is_prefix_expr(&self) -> bool {
        matches!(
            self,
            Expr::Var(_) | Expr::Index { .. } | Expr::Field { .. } | Expr::Call(_) | Expr::Paren(_)
        )
    }

    fn prec(&self) -> u8 {
        match self {
            Expr::Binary { op, .. } => op.prec(),
            Expr::Unary { .. } => PREC_UNARY,
            Expr::IfElse { .. } => PREC_IFELSE,
            _ => PREC_PRIMARY
        }
    }

    pub(crate) fn print_prefix(&self, out: &mut String, indent: usize) {
        if self.is_prefix_expr() {
            self.print_to(out, indent);
        } else {
            out.push('(');
            self.print_to(out, indent);
            out.push(')');
        }
    }

    /// Parenthesises only where a tighter operator would otherwise swallow a
    /// looser one, so the printed source parses back to this same tree. That
    /// matters for the reducer, which reads its own output again.
    fn print_prec(&self, out: &mut String, indent: usize, parent: u8) {
        let parens = self.prec() < parent;
        if parens {
            out.push('(');
        }
        match self {
            Expr::Literal(lit) => lit.print_to(out, indent),
            Expr::Var(name) => out.push_str(name),
            Expr::VarArgs => out.push_str("..."),
            Expr::Unary { op, operand } => {
                out.push_str(op.as_str());
                // `- -x` has to keep its parentheses or `--` starts a comment
                let inner = if matches!(op, UnOp::Neg) { PREC_UNARY + 1 } else { PREC_UNARY };
                operand.print_prec(out, indent, inner);
            },
            Expr::Binary { op, left, right } => {
                let prec = op.prec();
                let (left_prec, right_prec) = if op.is_right_assoc() {
                    (prec + 1, prec)
                } else {
                    (prec, prec + 1)
                };
                left.print_prec(out, indent, left_prec);
                out.push(' ');
                out.push_str(op.as_str());
                out.push(' ');
                right.print_prec(out, indent, right_prec);
            },
            // kept even when redundant: it truncates a call or `...` to one value
            Expr::Paren(inner) => {
                out.push('(');
                inner.print_to(out, indent);
                out.push(')');
            },
            Expr::Index { object, key } => {
                object.print_prefix(out, indent);
                out.push('[');
                key.print_to(out, indent);
                out.push(']');
            },
            Expr::Field { object, name } => {
                object.print_prefix(out, indent);
                out.push('.');
                out.push_str(name);
            },
            Expr::Call(call) => call.print_to(out, indent),
            Expr::Function(body) => {
                out.push_str("function");
                body.print_to(out, indent);
            },
            Expr::Table(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{ ");
                    print_list(fields, out, indent);
                    out.push_str(" }");
                }
            },
            Expr::IfElse { cond, then, else_ } => {
                out.push_str("if ");
                cond.print_prec(out, indent, PREC_OR);
                out.push_str(" then ");
                then.print_prec(out, indent, PREC_OR);
                out.push_str(" else ");
                else_.print_prec(out, indent, PREC_OR);
            }
        }
        if parens {
            out.push(')');
        }
    }
}

impl Printer for Expr {
    fn print_to(&self, out: &mut String, indent: usize) {
        self.print_prec(out, indent, PREC_IFELSE);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprKind {
    Literal,
    Var,
    Unary,
    Binary,
    Paren,
    Call,
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

impl Printer for LValue {
    fn print_to(&self, out: &mut String, indent: usize) {
        match self {
            Self::Var(name) => out.push_str(name),
            Self::Index { object, key } => {
                object.print_prefix(out, indent);
                out.push('[');
                key.print_to(out, indent);
                out.push(']');
            },
            Self::Field { object, name } => {
                object.print_prefix(out, indent);
                out.push('.');
                out.push_str(name);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LValueKind {
    Var, Index, Field
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalName {
    pub name: Ident,
    pub annotation: Option<TypeAnnot>,
}

impl Printer for LocalName {
    fn print_to(&self, out: &mut String, indent: usize) {
        out.push_str(&self.name);
        if let Some(annot) = &self.annotation {
            out.push_str(": ");
            annot.print_to(out, indent);
        }
    }
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

    PrintGlobals(Vec<Ident>)
}

impl Printer for Stmt {
    /// Starts on a line the caller has already indented, and never writes a
    /// trailing newline. Anything with a body closes at the caller's level.
    fn print_to(&self, out: &mut String, indent: usize) {
        match self {
            Self::Local { names, values } => {
                out.push_str("local ");
                print_list(names, out, indent);
                if !values.is_empty() {
                    out.push_str(" = ");
                    print_list(values, out, indent);
                }
            },
            Self::Assign { targets, values } => {
                print_list(targets, out, indent);
                out.push_str(" = ");
                print_list(values, out, indent);
            },
            Self::CompoundAssign { target, op, value } => {
                target.print_to(out, indent);
                out.push(' ');
                out.push_str(op.as_str());
                out.push(' ');
                value.print_to(out, indent);
            },
            Self::LocalFunction { name, body } => {
                out.push_str("local function ");
                out.push_str(name);
                body.print_to(out, indent);
            },
            Self::Function { name, body } => {
                out.push_str("function ");
                out.push_str(name);
                body.print_to(out, indent);
            },
            Self::Call(call) => call.print_to(out, indent),
            Self::Do(block) => {
                out.push_str("do\n");
                block.print_to(out, indent + 1);
                write_indent(out, indent);
                out.push_str("end");
            },
            Self::While { cond, body } => {
                out.push_str("while ");
                cond.print_to(out, indent);
                out.push_str(" do\n");
                body.print_to(out, indent + 1);
                write_indent(out, indent);
                out.push_str("end");
            },
            Self::Repeat { body, until } => {
                out.push_str("repeat\n");
                body.print_to(out, indent + 1);
                write_indent(out, indent);
                out.push_str("until ");
                until.print_to(out, indent);
            },
            Self::If { cond, then, elseifs, else_ } => {
                out.push_str("if ");
                cond.print_to(out, indent);
                out.push_str(" then\n");
                then.print_to(out, indent + 1);
                for (cond, block) in elseifs {
                    write_indent(out, indent);
                    out.push_str("elseif ");
                    cond.print_to(out, indent);
                    out.push_str(" then\n");
                    block.print_to(out, indent + 1);
                }
                if let Some(block) = else_ {
                    write_indent(out, indent);
                    out.push_str("else\n");
                    block.print_to(out, indent + 1);
                }
                write_indent(out, indent);
                out.push_str("end");
            },
            Self::NumericFor { var, start, limit, step, body } => {
                out.push_str("for ");
                out.push_str(var);
                out.push_str(" = ");
                start.print_to(out, indent);
                out.push_str(", ");
                limit.print_to(out, indent);
                if let Some(step) = step {
                    out.push_str(", ");
                    step.print_to(out, indent);
                }
                out.push_str(" do\n");
                body.print_to(out, indent + 1);
                write_indent(out, indent);
                out.push_str("end");
            },
            Self::GenericFor { vars, iter, body } => {
                out.push_str("for ");
                print_idents(vars, out);
                out.push_str(" in ");
                print_list(iter, out, indent);
                out.push_str(" do\n");
                body.print_to(out, indent + 1);
                write_indent(out, indent);
                out.push_str("end");
            },
            // every value goes through the dumper: printing a table directly
            // would emit its address. Split across calls because every argument
            // costs a register, and a function only has 255 of them
            Self::PrintGlobals(names) => {
                if names.is_empty() {
                    out.push_str("print()");
                    return;
                }
                for (i, batch) in names.chunks(PRINT_BATCH).enumerate() {
                    if i > 0 {
                        out.push('\n');
                        write_indent(out, indent);
                    }
                    out.push_str("print(");
                    for (j, name) in batch.iter().enumerate() {
                        if j > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(DUMP_FN);
                        out.push('(');
                        out.push_str(name);
                        out.push(')');
                    }
                    out.push(')');
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Local, Assign, CompoundAssign,
    LocalFunction, Function, Call,
    Do, While, Repeat,
    If, NumericFor, GenericFor,
    PrintGlobals
}

/// Statements that may only appear last in a block.
#[derive(Debug, Clone, PartialEq)]
pub enum LastStmt {
    Return(Vec<Expr>),
    Break,
    Continue,
}

impl Printer for LastStmt {
    fn print_to(&self, out: &mut String, indent: usize) {
        match self {
            Self::Return(values) => {
                out.push_str("return");
                if !values.is_empty() {
                    out.push(' ');
                    print_list(values, out, indent);
                }
            },
            Self::Break => out.push_str("break"),
            Self::Continue => out.push_str("continue")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub last: Option<LastStmt>,
}

impl Printer for Block {
    /// One statement per line, each at `indent`.
    fn print_to(&self, out: &mut String, indent: usize) {
        for stmt in &self.stmts {
            write_indent(out, indent);
            let start = out.len();
            stmt.print_to(out, indent);
            // a statement opening with `(` would keep the line above going:
            // `f\n(x)()` is one call spread over two lines, not two statements
            if out.as_bytes().get(start) == Some(&b'(') {
                out.insert(start, ';');
            }
            out.push('\n');
        }
        if let Some(last) = &self.last {
            write_indent(out, indent);
            last.print_to(out, indent);
            out.push('\n');
        }
    }
}

/// A whole generated program. The chunk is itself a vararg function, so
/// top-level `return` and `...` are legal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub body: Block,
}

impl Printer for Program {
    fn print_to(&self, out: &mut String, indent: usize) {
        if self.body.stmts.iter().any(|stmt| matches!(stmt, Stmt::PrintGlobals(_))) {
            out.push_str(DUMP_PRELUDE);
        }
        self.body.print_to(out, indent);
    }
}