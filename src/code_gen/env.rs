use std::collections::HashSet;
use crate::code_gen::ast::Ident;
use rand;

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

#[derive(Debug, Clone)]
pub struct Env {
    /// Every variable in scope, innermost last. Walk from the end and skip
    /// names already seen to get correct shadowing.
    vars: Vec<Var>,
    /// Open scopes, outermost first. The chunk itself is the first frame
    /// and is a vararg `Function` frame.
    frames: Vec<Frame>,
    /// Assigned globals. Visible everywhere regardless of frames, and they
    /// compile to different bytecode than locals or upvalues.
    globals: Vec<Var>,

    next_id: u32
}

impl Env {
    pub fn new() -> Self {
        Self {
            vars: Vec::default(),
            frames: vec![Frame { kind: FrameKind::Block, start: 0 }],
            globals: Vec::default(),
            next_id: 0
        }
    }
    pub fn define_var(&mut self, name: &Ident, ty: Ty, kind: VarKind) {
        self.vars.push(Var {
            name: name.clone(),
            ty,
            kind,
            frame: self.frames.len() - 1,
            reassigned: false
        });
    }

    pub fn define_global(&mut self, name: &Ident, ty: Ty) {
        self.globals.push(Var {
            name: name.clone(),
            ty,
            kind: VarKind::Local,
            frame: self.frames.len() - 1,
            reassigned: false
        });
    }

    pub fn new_frame(&mut self, kind: FrameKind) {
        self.frames.push(Frame {
            kind, start: self.vars.len()
        });
    }

    pub fn close_frame(&mut self) {
        assert!(self.frames.len() > 1, "cannot close the chunk frame");
        let start = self.frames.pop().unwrap().start;
        self.vars.truncate(start);
    }

    fn fresh_name(&mut self, prefix: char) -> String {
        self.next_id += 1;
        format!("{}{}", prefix, self.next_id)
    }

    pub fn fresh_var_name(&mut self) -> String {
        self.fresh_name('v')
    }

    pub fn fresh_global_name(&mut self) -> String {
        self.fresh_name('G')
    }

    pub fn get_lvalue_of(&self, ty: &Ty) -> Option<&Var> {
        let mut seen = HashSet::new();
        self.vars
            .iter()
            .rev()
            .filter(|v| seen.insert(&v.name))
            .find(|v| v.ty == *ty)
    }

    /// Every variable visible from here, innermost first.
    ///
    /// No shadowing check, and no allocation: `fresh_name` hands out a new name
    /// every time, so two variables in scope can never share one. Reusing names
    /// would mean walking back to the innermost of each, which is what the
    /// `HashSet` in `get_lvalue_of` does.
    fn visible(&self) -> impl Iterator<Item = &Var> {
        self.vars.iter().rev().chain(self.globals.iter())
    }

    pub fn has_var_matching(&self, test: impl Fn(&Ty) -> bool) -> bool {
        self.visible().any(|var| test(&var.ty))
    }

    /// Counts the candidates, then walks to the one it picked. Two passes and a
    /// single random draw, against one allocation per call: this runs once per
    /// expression node, so it is worth the second pass.
    pub fn random_var_matching(&self, test: impl Fn(&Ty) -> bool) -> Option<&Var> {
        let matches = self.visible().filter(|var| test(&var.ty)).count();
        if matches == 0 {
            return None;
        }
        self.visible()
            .filter(|var| test(&var.ty))
            .nth(rand::random_range(0..matches))
    }
    
    pub fn get_globals(&self) -> Vec<Var> {
        self.globals.clone()
    }
}