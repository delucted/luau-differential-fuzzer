use crate::code_gen::ast::Ident;

/// The static type the generator believes a variable holds. This is the
/// generator's bookkeeping, not Luau's type system, and it may be wrong on
/// purpose when testing type-directed optimisations.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Number,
    String,
    Boolean,
    Nil,
    /// Array-shaped table with elements of one type. Hash-shaped tables are
    /// not modelled in v0: iterating them is nondeterministic and they add
    /// little the array case does not.
    Array(Box<Ty>),
    Function(FnSig),
    /// Unknown or deliberately mixed. Only usable where any value is fine:
    /// `print`, `tostring`, `type`, `==`, truthiness tests.
    Any,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub params: Vec<Ty>,
    pub is_vararg: bool,
    /// Return types by position. Empty means the function returns nothing.
    pub ret: Vec<Ty>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Local,
    Param,
    /// Numeric or generic for-loop variable. A fresh local per iteration.
    LoopVar,
    /// Declared with `local function`, in scope inside its own body.
    LocalFunction,
    Global,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Var {
    pub name: Ident,
    pub ty: Ty,
    pub kind: VarKind,
    /// Index into `Env::frames` of the scope that declared this variable.
    /// A variable is an upvalue at the current position if this index is
    /// below the innermost `FrameKind::Function` frame.
    pub frame: usize,
    /// Set once the variable is assigned after its declaration. A local
    /// initialised with a literal and never reassigned is what O2
    /// constant-folds; keep some of them that way.
    pub reassigned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FrameKind {
    /// `do`, `if` branches, and anything else that only introduces a scope.
    Block,
    /// `while`, `repeat`, numeric and generic `for`. Makes `break` and
    /// `continue` legal, but only until the next `Function` frame.
    Loop,
    /// A function body. Resets what `break` may target, decides whether
    /// `...` is legal, and fixes the shape `return` must produce.
    Function { is_vararg: bool, ret: Vec<Ty> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub kind: FrameKind,
    /// `Env::vars.len()` at the moment this frame was opened. Closing the
    /// frame truncates `vars` back to this length.
    pub start: usize,
}

#[derive(Debug, Default)]
pub struct Env {
    /// Every variable in scope, innermost last. Walk from the end and skip
    /// names already seen to get correct shadowing.
    pub vars: Vec<Var>,
    /// Open scopes, outermost first. The chunk itself is the first frame
    /// and is a vararg `Function` frame.
    pub frames: Vec<Frame>,
    /// Assigned globals. Visible everywhere regardless of frames, and they
    /// compile to different bytecode than locals or upvalues.
    pub globals: Vec<Var>,
    /// Program-wide counter for fresh identifiers, so names are unique
    /// without a lookup.
    pub next_id: u32,
}