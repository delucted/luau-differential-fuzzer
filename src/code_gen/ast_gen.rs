//! Engine for generating random Luau ASTs, with a goal
//! of disgustingness.

use crate::code_gen::ast::*;
use crate::code_gen::env::*;
use crate::code_gen::costs::DEFAULT_COSTS;
use rand;
use rand::seq::{IndexedRandom};

const MAX_STMT_MISSES: u32 = 10;

pub struct AstGenerator {
    fuel: i32,
    expr_depth: i32,
    function_depth: i32,
    env: Env,
}

impl AstGenerator {
    pub fn new(fuel: u32) -> Self {
        Self {
            fuel: fuel as i32,
            expr_depth: 0,
            function_depth: 0,
            env: Env::new()
        }
    }

    fn use_fuel(&mut self, amount: i32) -> Result<(), ()> {
        if self.fuel - amount <= 0 {
            return Err(())
        }
        self.fuel -= amount;
        Ok(())
    }

    fn gen_number() -> Literal {
        Literal::Number(match rand::random_range(1..=7) { // choose from arsenal of nastiness
            1 => rand::random_range(0f64..=f64::MAX),
            2 => 0f64,
            3 => 1f64,
            4 => -1f64,
            5 => 2f64.powi(31),
            6 => 2f64.powi(53),
            7 => 0.1,
            _ => 0f64
        })
    }

    fn gen_rand_string(len: u8) -> String {
        let mut str: Vec<u8> = vec![];
        for _ in 0..len {
            str.push('a' as u8);
        }
        String::from_utf8(str).unwrap()
    }

    fn gen_string() -> Literal {
        match rand::random_range(1..=3) {
            1 => {
                let str = Self::gen_rand_string(255);
                Literal::String(str)
            },
            2 => Literal::String(String::from("")),
            3 => Literal::String(
                Self::gen_rand_string(
                    rand::random_range(1..=10)
                )
            ),
            _ => Literal::Nil
        }
    }

    fn gen_boolean() -> Literal {
        match rand::random_range(1..=2) {
            1 => Literal::Boolean(false),
            2 => Literal::Boolean(true),
            _ => Literal::Nil
        }
    }

    fn gen_annotation(honest: &Ty) -> TypeAnnot {
        TypeAnnot::Any // TODO: implement TypeAnnot gen
    }

    fn gen_local_name(&mut self, honest: &Ty) -> LocalName {
        LocalName {
            name: if self.function_depth == 0 {
                self.env.fresh_global_name()
            } else {
                self.env.fresh_var_name()
            },
            annotation: None
        }
    }

    fn gen_random_literal(want: &Ty) -> Literal {
        match want {
            Ty::Nil => Literal::Nil,
            Ty::Number => Self::gen_number(),
            Ty::Boolean => Self::gen_boolean(),
            Ty::String => Self::gen_string(),
            // `Any`: the literal decides its own type
            _ => match rand::random_range(1..=4) {
                1 => Literal::Nil,
                2 => Self::gen_number(),
                3 => Self::gen_boolean(),
                _ => Self::gen_string()
            }
        }
    }

    /// A type for a position that does not care which one it gets.
    fn gen_ty() -> Ty {
        match rand::random_range(1..=6) {
            1 => Ty::Number,
            2 => Ty::String,
            3 => Ty::Boolean,
            4 => Ty::Nil,
            5 => Ty::Array(Box::from(Self::gen_element_ty())),
            _ => Ty::Any
        }
        // TODO: Ty::Function, once a body can be generated to match a signature
    }

    /// Element types stay scalar, so a type cannot just nest infinitely.
    fn gen_element_ty() -> Ty {
        match rand::random_range(1..=4) {
            1 => Ty::Number,
            2 => Ty::String,
            3 => Ty::Boolean,
            _ => Ty::Any
        }
    }

    /// Get the expression forms that can produce a `want` in the current state
    fn get_avail(&self, want: &Ty) -> Vec<ExprKind> {
        // the leaf forms first. Neither has an expression under it, so both
        // are still allowed once the depth cap is reached
        let mut avail: Vec<ExprKind> = Vec::new();
        if self.env.has_var_matching(|ty| Self::satisfies(ty, want)) {
            avail.push(ExprKind::Var);
        }
        if !matches!(want, Ty::Array(_) | Ty::Function(_)) {
            avail.push(ExprKind::Literal); // no literal writes a table or a function
        }

        if self.expr_depth >= DEFAULT_COSTS.max_expr_depth {
            if avail.is_empty() {
                // a table or a function with no variable to read it from has to
                // be built; its contents are a level deeper, so they are leaves
                avail.push(Self::leaf_kind(want));
            }
            return avail;
        }

        // these two pass the demand straight through, so they fit any `want`
        avail.extend([ExprKind::Paren, ExprKind::IfElse]);
        match want {
            Ty::Number => avail.extend([ExprKind::Unary, ExprKind::Binary]),
            Ty::String => avail.push(ExprKind::Binary),
            Ty::Boolean => avail.extend([ExprKind::Unary, ExprKind::Binary]),
            Ty::Nil => {},
            Ty::Array(_) => avail.push(ExprKind::Table),
            Ty::Function(_) => avail.push(ExprKind::Function),
            Ty::Any => {
                avail.extend([ExprKind::Unary, ExprKind::Binary, ExprKind::Table]);
                if self.function_depth < DEFAULT_COSTS.max_function_depth {
                    avail.push(ExprKind::Function);
                }
            }
        }

        avail // TODO: Call, Index and Field, once calls and reads are generated
    }

    /// The only form that can produce a `want` with no expression under it,
    /// for when the depth cap is reached.
    fn leaf_kind(want: &Ty) -> ExprKind {
        match want {
            Ty::Array(_) => ExprKind::Table,
            Ty::Function(_) => ExprKind::Function,
            _ => ExprKind::Literal
        }
    }

    /// `-` and `#` produce a number, `not` produces a boolean.
    fn un_ops_for(want: &Ty) -> Vec<UnOp> {
        match want {
            Ty::Number => vec![UnOp::Neg, UnOp::Len],
            Ty::Boolean => vec![UnOp::Not],
            _ => vec![UnOp::Neg, UnOp::Not, UnOp::Len]
        }
    }

    fn gen_unary_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.unary)?;
        let op = *Self::un_ops_for(want).choose(&mut rand::rng()).unwrap();
        let operand = match op {
            UnOp::Neg => self.gen_expr(&Ty::Number)?,
            UnOp::Not => self.gen_expr(&Ty::Any)?, // anything is truthy or falsy
            // `#` takes a string or a table
            UnOp::Len => match rand::random_range(1..=2) {
                1 => self.gen_expr(&Ty::String)?,
                _ => self.gen_expr(&Ty::Array(Box::from(Self::gen_element_ty())))?
            }
        };
        Ok(Expr::Unary {
            op,
            operand: Box::from(operand)
        })
    }

    /// Arithmetic produces a number, `..` a string, comparisons a boolean.
    /// `and` and `or` produce one of their operands, so they only fit where
    /// any value is acceptable.
    fn bin_ops_for(want: &Ty) -> Vec<BinOp> {
        let arith = [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::FloorDiv, BinOp::Mod, BinOp::Pow];
        let compare = [BinOp::Eq, BinOp::Ne, BinOp::Lt, BinOp::Le, BinOp::Gt, BinOp::Ge];
        match want {
            Ty::Number => arith.to_vec(),
            Ty::String => vec![BinOp::Concat],
            Ty::Boolean => compare.to_vec(),
            _ => {
                let mut ops = arith.to_vec();
                ops.extend(compare);
                ops.extend([BinOp::Concat, BinOp::And, BinOp::Or]);
                ops
            }
        }
    }

    fn gen_binary_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.binary)?;
        let op = *Self::bin_ops_for(want).choose(&mut rand::rng()).unwrap();
        // both sides share a type: `<` on mixed operands is a runtime error and
        // `==` on mixed operands is just always false
        let operand_ty = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
            | BinOp::FloorDiv | BinOp::Mod | BinOp::Pow => Ty::Number,
            // `..` takes numbers too, and turns them into strings
            BinOp::Concat | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                match rand::random_range(1..=2) {
                    1 => Ty::Number,
                    _ => Ty::String
                }
            },
            BinOp::Eq | BinOp::Ne => Self::gen_ty(),
            BinOp::And | BinOp::Or => Ty::Any
        };
        Ok(Expr::Binary {
            op,
            left: Box::from(self.gen_expr(&operand_ty)?),
            right: Box::from(self.gen_expr(&operand_ty)?)
        })
    }

    fn gen_paren_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.paren)?;
        Ok(Expr::Paren(Box::from(self.gen_expr(want)?)))
    }

    fn gen_function_body(&mut self) -> Result<FunctionBody, ()> {
        let mut params: Vec<Param> = vec![];
        loop {
            if let 2 = rand::random_range(1..=2) {
                break;
            }
            self.use_fuel(DEFAULT_COSTS.param)?;
            params.push(
                Param {
                    name: self.env.fresh_var_name(),
                    annotation: None
                }
            )
        }
        let is_vararg = match rand::random_range(1..=2) {
            1 => false,
            2 => true,
            _ => false
        };
        self.function_depth += 1;
        self.env.new_frame(FrameKind::Function { is_vararg, ret: vec![] });
        let body = self.gen_block();
        self.env.close_frame();
        self.function_depth -= 1;
        Ok(FunctionBody {
            params, is_vararg, body: body?
        })
    }

    fn gen_function_expr(&mut self) -> Result<Expr, ()> {
        // TODO: take a `want` and honour its FnSig. Nothing demands a function
        // type yet, because a body cannot return to order until LastStmt is
        // generated, so every function here is an unconstrained one.
        self.use_fuel(DEFAULT_COSTS.function_expr)?;
        Ok(Expr::Function(
            self.gen_function_body()?
        ))
    }

    /// An `Array(t)` is array-shaped by definition, so it takes positional
    /// fields of `t` and nothing else. A table nobody has a type for can have
    /// any shape.
    fn gen_table_field(&mut self, want: &Ty) -> Result<TableField, ()> {
        if let Ty::Array(elem) = want {
            self.use_fuel(DEFAULT_COSTS.table_field_positional)?;
            return Ok(TableField::Positional(
                self.gen_expr(elem)?
            ));
        }
        match rand::random_range(1..=3) {
            1 => {
                self.use_fuel(DEFAULT_COSTS.table_field_positional)?;
                Ok(TableField::Positional(
                    self.gen_expr(&Self::gen_ty())?
                ))
            },
            2 => {
                self.use_fuel(DEFAULT_COSTS.table_field_named)?;
                Ok(TableField::Named {
                    name: self.env.fresh_var_name(),
                    value: self.gen_expr(&Self::gen_ty())?,
                })
            },
            _ => {
                self.use_fuel(DEFAULT_COSTS.table_field_keyed)?;
                Ok(TableField::Keyed {
                    // a computed number key can come out NaN, which is a
                    // runtime error, so keys are strings for now
                    key: self.gen_expr(&Ty::String)?,
                    value: self.gen_expr(&Self::gen_ty())?,
                })
            }
        }
    }

    fn gen_table_fields(&mut self, want: &Ty) -> Result<Vec<TableField>, ()> {
        let mut table_fields: Vec<TableField> = vec![];
        loop {
            if let 2 = rand::random_range(1..=2) {
                break;
            }
            table_fields.push(
                self.gen_table_field(want)?
            )
        }
        Ok(table_fields)
    }

    fn gen_table_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.table)?;
        Ok(Expr::Table(
            self.gen_table_fields(want)?
        ))
    }

    fn gen_ifelse_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.if_else_expr)?;
        Ok(Expr::IfElse {
            cond: Box::from(self.gen_expr(&Ty::Any)?), // any value is truthy or falsy
            then: Box::from(self.gen_expr(want)?),
            else_: Box::from(self.gen_expr(want)?)
        })
    }

    fn gen_literal_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.literal)?;
        Ok(Expr::Literal(Self::gen_random_literal(want)))
    }

    /// Whether a variable of type `ty` can be read where a `want` is demanded.
    /// A demand for `Any` takes anything, but an `Any` variable satisfies
    /// nothing else: once the generator has lost track of a value, it can only
    /// go where any value is fine.
    fn satisfies(ty: &Ty, want: &Ty) -> bool {
        matches!(want, Ty::Any) || ty == want
    }

    fn gen_var_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.var)?;
        Ok(Expr::Var(
            self.env.random_var_matching(|ty| Self::satisfies(ty, want))
                .ok_or(())?
                .name
                .clone()
        ))
    }

    fn gen_expr(&mut self, want: &Ty) -> Result<Expr, ()> {
        let chosen_expr = *self.get_avail(want).choose(&mut rand::rng()).unwrap();
        self.expr_depth += 1;
        let expr = match chosen_expr {
            ExprKind::Literal => self.gen_literal_expr(want),
            ExprKind::Var => self.gen_var_expr(want),
            ExprKind::Unary => self.gen_unary_expr(want),
            ExprKind::Binary => self.gen_binary_expr(want),
            ExprKind::Paren => self.gen_paren_expr(want),
            ExprKind::Function => self.gen_function_expr(),
            ExprKind::Table => self.gen_table_expr(want),
            ExprKind::IfElse => self.gen_ifelse_expr(want)
        };
        self.expr_depth -= 1;
        expr
    }

    fn gen_local(&mut self) -> Result<Stmt, ()> {
        // the type of each name is picked first, so the values can be generated
        // to match and the annotations can be honest about them
        let mut tys: Vec<Ty> = Vec::new();
        loop { // a local needs at least one name
            self.use_fuel(DEFAULT_COSTS.local)?;
            tys.push(Self::gen_ty());
            if let 2 = rand::random_range(1..=2) {
                break;
            }
        }
        let name_count = tys.len();
        let mut values: Vec<Expr> = Vec::new();
        let mut i = 0;
        loop {
            i += 1;
            // 10% chance of fewer values
            if !(i == 1) {
                if let 1 = rand::random_range(1..=10) {
                    break;
                }
            }
            if i >= name_count {
                // 20% chance of extra values
                if let 1 = rand::random_range(1..=5) {}
                else {
                    break;
                }
            }
            // a value past the last name is discarded, so its type is free
            let want = tys.get(values.len()).cloned().unwrap_or(Ty::Any);
            values.push(self.gen_expr(&want)?);
        }
        let mut names: Vec<LocalName> = Vec::new();
        for (i, ty) in tys.iter().enumerate() {
            // a name with no value of its own is nil
            let honest = if i < values.len() { ty.clone() } else { Ty::Nil };
            let name = self.gen_local_name(&honest);
            if self.function_depth == 0 {
                self.env.define_global(&name.name, honest);
            }
            names.push(name);
        }
        Ok(Stmt::Local {
            names,
            values
        })
    }

    fn get_lvalue(&mut self, test: fn(ty: &Ty) -> bool, allow_index: bool) -> Result<(LValue, Ty), ()> {
        let indexable = |ty: &Ty| matches!(ty, Ty::Array(elem) if test(elem));

        let mut kinds = vec![LValueKind::Var];
        if allow_index && self.env.has_var_matching(indexable) {
            kinds.push(LValueKind::Index);
        }
        // TODO: LValueKind::Field, once `Ty` can describe a table's named fields
        match kinds.choose(&mut rand::rng()).unwrap() {
            LValueKind::Index => {
                self.use_fuel(DEFAULT_COSTS.index)?;
                let var = self.env
                    .random_var_matching(indexable)
                    .ok_or(())?;
                let elem = match &var.ty {
                    Ty::Array(elem) => elem.as_ref().clone(),
                    _ => Ty::Any
                };
                let object = Expr::Var(var.name.clone());

                let key = match rand::random_range(1..=2) {
                    1 => Expr::Literal(Literal::Number(1f64)),
                    _ => Expr::Binary {
                        op: BinOp::Add,
                        left: Box::from(Expr::Unary {
                            op: UnOp::Len,
                            operand: Box::from(object.clone())
                        }),
                        right: Box::from(Expr::Literal(Literal::Number(1f64)))
                    }
                };
                Ok((LValue::Index { object, key }, elem))
            },
            _ => {
                let var = self.env
                    .random_var_matching(|ty| !matches!(ty, Ty::Function(_)) && test(ty))
                    .ok_or(())?;
                Ok((LValue::Var(var.name.clone()), var.ty.clone()))
            }
        }
    }

    fn gen_assign(&mut self) -> Result<Stmt, ()> {
        let mut targets: Vec<(LValue, Ty)> = vec![];
        loop { // gen targets
            self.use_fuel(DEFAULT_COSTS.assign)?;
            let v = self.get_lvalue(|_| true, true)?;
            targets.push(v);
            if let 1 = rand::random_range(1..=2) {
                break;
            }
        }
        let mut values: Vec<Expr> = vec![];
        let mut t: Vec<LValue> = vec![];
        for (target, ty) in targets {
            t.push(target);
            values.push(self.gen_expr(&ty)?);
        }
        Ok(Stmt::Assign {
            targets: t,
            values
        })
    }

    fn get_numeric_compound_op(&mut self) -> CompoundOp {
        match rand::random_range(1..=7) {
            1 => CompoundOp::Add,
            2 => CompoundOp::Sub,
            3 => CompoundOp::Mul,
            4 => CompoundOp::Div,
            5 => CompoundOp::FloorDiv,
            6 => CompoundOp::Mod,
            _ => CompoundOp::Pow
        }
    }

    fn gen_compound_assign(&mut self) -> Result<Stmt, ()> {
        self.use_fuel(DEFAULT_COSTS.compound_assign)?;
        let v = self.get_lvalue(
            |ty| matches!(ty, Ty::Number | Ty::String), false
        )?;
        Ok(Stmt::CompoundAssign {
            target: v.0,
            op: match v.1 {
                Ty::String => CompoundOp::Concat,
                _ => self.get_numeric_compound_op()
            },
            value: self.gen_expr(&v.1)?
        })
    }

    fn avail_stmts(&mut self) -> Vec<StmtKind> {
        let mut stmt_kinds = vec![StmtKind::Local];
        if self.env.has_var_matching(|_| true) {  // has any lvalue
            stmt_kinds.push(StmtKind::Assign);
        }
        if self.env.has_var_matching(|ty| matches!(ty, Ty::Number | Ty::String)) {
            stmt_kinds.push(StmtKind::CompoundAssign);
        }
        stmt_kinds
    }

    fn gen_stmt(&mut self) -> Result<Stmt, ()> {
        let chosen_stmt = self.avail_stmts()
            .choose(&mut rand::rng())
            .unwrap()
            .clone();
        match chosen_stmt {
            StmtKind::Local => self.gen_local(),
            StmtKind::Assign => self.gen_assign(),
            _ => self.gen_compound_assign(),
            // StmtKind::CompoundAssign
        }
    }

    fn gen_block(&mut self) -> Result<Block, ()> {
        let mut stmts: Vec<Stmt> = Vec::new();
        loop {
            if let 2 = rand::random_range(1..=2) {
                break;
            }
            stmts.push(self.gen_stmt()?);
        }

        Ok(Block {
            stmts,
            last: None
        })
    }

    fn gen_program(&mut self) -> Program {
        let mut stmts: Vec<Stmt> = Vec::new();

        // a statement that runs out of fuel is dropped and refunded, then we try a (hopefully smaller) one.
        // once enough attempts in a row fail, we're out of fuel for good
        let mut misses = 0;
        while misses < MAX_STMT_MISSES {
            let fuel = self.fuel;
            match self.gen_stmt() {
                Ok(stmt) => {
                    stmts.push(stmt);
                    misses = 0;
                },
                Err(()) => {
                    self.fuel = fuel;
                    misses += 1;
                }
            }
        }

        let mut globals: Vec<Ident> = vec![];
        for global in self.env.get_globals() {
            globals.push(global.name);
        }
        stmts.push(Stmt::PrintGlobals(globals));
        Program {
            body: Block {
                stmts,
                last: None
            }
        }
    }

    pub fn gen_ast(&mut self) -> Program {
        self.gen_program()
    }
}
