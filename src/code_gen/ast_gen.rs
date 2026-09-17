use crate::code_gen::ast::*;
use crate::code_gen::costs::DEFAULT_COSTS;
use crate::code_gen::ident_gen::IdentGen;
use rand;
use rand::seq::{IndexedRandom};

const MAX_STMT_MISSES: u32 = 10;

pub struct AstGenerator {
    fuel: i32,
    expr_depth: i32,
    function_depth: i32,
    ident_gen: IdentGen
}

impl AstGenerator {
    pub fn new(fuel: u32) -> Self {
        Self { fuel: fuel as i32, expr_depth: 0, function_depth: 0, ident_gen: IdentGen::new() }
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
            str.push(rand::random_range(32..=126));
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
                    rand::random_range(1..=u8::MAX)
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

    fn gen_annotation(honest: Literal) -> TypeAnnot {
        TypeAnnot::Any // TODO: implement TypeAnnot gen
    }

    fn gen_local_name(&mut self, kind: Literal) -> LocalName {
        LocalName {
            name: self.ident_gen.gen_ident(),
            annotation: Some(Self::gen_annotation(kind))
        }
    }

    fn gen_random_literal() -> Literal {
        match rand::random_range(1..=4) {
            1 => Literal::Nil,
            2 => Self::gen_number(),
            3 => Self::gen_boolean(),
            4 => Self::gen_string(),
            _ => Literal::Nil
        }
    }

    fn get_avail(&self) -> Vec<u8> {
        if self.expr_depth >= DEFAULT_COSTS.max_expr_depth {
            return vec![0];
        }

        let mut avail: Vec<u8> = vec![0, 3, 4, 5, 10, 11];
        if self.function_depth < DEFAULT_COSTS.max_function_depth {
            avail.push(9);
        }

        avail // TODO: implement availability
    }

    fn gen_un_op() -> UnOp {
        match rand::random_range(1..=3) {
            1 => UnOp::Neg,
            2 => UnOp::Not,
            3 => UnOp::Len,
            _=>{ UnOp::Neg }
        }
    }

    fn gen_unary_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.unary)?;
        Ok(Expr::Unary {
            op: Self::gen_un_op(),
            operand: Box::from(self.gen_expr()?)
        })
    }

    fn gen_bin_op() -> BinOp {
        match rand::random_range(0..=15) {
            0 => BinOp::Add,
            1 => BinOp::Sub,
            2 => BinOp::Mul,
            3 => BinOp::Div,
            4 => BinOp::FloorDiv,
            5 => BinOp::Mod,
            6 => BinOp::Pow,
            7 => BinOp::Concat,
            8 => BinOp::Eq,
            9 => BinOp::Ne,
            10 => BinOp::Lt,
            11 => BinOp::Le,
            12 => BinOp::Gt,
            13 => BinOp::Ge,
            14 => BinOp::And,
            15 => BinOp::Or,
            _ => BinOp::Add
        }
    }

    fn gen_binary_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.binary)?;
        Ok(Expr::Binary {
            op: Self::gen_bin_op(),
            left: Box::from(self.gen_expr()?),
            right: Box::from(self.gen_expr()?)
        })
    }

    fn gen_paren_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.paren)?;
        Ok(Expr::Paren(Box::from(self.gen_expr()?)))
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
                    name: self.ident_gen.gen_ident(),
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
        let body = self.gen_block();
        self.function_depth -= 1;
        Ok(FunctionBody {
            params, is_vararg, body: body?
        })
    }

    fn gen_function_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.function_expr)?;
        Ok(Expr::Function(
            self.gen_function_body()?
        ))
    }

    fn gen_table_field(&mut self) -> Result<TableField, ()> {
        match rand::random_range(1..=3) {
            1 => {
                self.use_fuel(DEFAULT_COSTS.table_field_positional)?;
                Ok(TableField::Positional(
                    self.gen_expr()?
                ))
            },
            2 => {
                self.use_fuel(DEFAULT_COSTS.table_field_named)?;
                Ok(TableField::Named {
                    name: self.ident_gen.gen_ident(),
                    value: self.gen_expr()?,
                })
            },
            _ => {
                self.use_fuel(DEFAULT_COSTS.table_field_keyed)?;
                Ok(TableField::Keyed {
                    key: self.gen_expr()?,
                    value: self.gen_expr()?,
                })
            }
        }
    }

    fn gen_table_fields(&mut self) -> Result<Vec<TableField>, ()> {
        let mut table_fields: Vec<TableField> = vec![];
        loop {
            if let 2 = rand::random_range(1..=2) {
                break;
            }
            table_fields.push(
                self.gen_table_field()?
            )
        }
        Ok(table_fields)
    }

    fn gen_table_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.table)?;
        Ok(Expr::Table(
            self.gen_table_fields()?
        ))
    }

    fn gen_ifelse_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.if_else_expr)?;
        Ok(Expr::IfElse {
            cond: Box::from(self.gen_expr()?),
            then: Box::from(self.gen_expr()?),
            else_: Box::from(self.gen_expr()?)
        })
    }

    fn gen_literal_expr(&mut self) -> Result<Expr, ()> {
        self.use_fuel(DEFAULT_COSTS.literal)?;
        Ok(Expr::Literal(Self::gen_random_literal()))
    }

    fn gen_expr(&mut self) -> Result<Expr, ()> {
        let chosen_expr = *self.get_avail().choose(&mut rand::rng()).unwrap();
        // no early returns between these, so the depth is restored even when out of fuel
        self.expr_depth += 1;
        let expr = match chosen_expr {
            0 => self.gen_literal_expr(),
            3 => self.gen_unary_expr(),
            4 => self.gen_binary_expr(),
            5 => self.gen_paren_expr(),
            9 => self.gen_function_expr(),
            10 => self.gen_table_expr(),
            11 => self.gen_ifelse_expr(),
            _ => { Ok(Expr::Literal(Literal::Nil)) }
        };
        self.expr_depth -= 1;
        expr
    }

    fn gen_local(&mut self) -> Result<Stmt, ()> {
        let mut names: Vec<LocalName> = Vec::new();
        loop { // a local needs at least one name
            self.use_fuel(DEFAULT_COSTS.local)?;
            names.push(self.gen_local_name(Self::gen_random_literal()));
            if let 2 = rand::random_range(1..=2) {
                break;
            }
        }
        let mut values: Vec<Expr> = Vec::new();
        loop {
            if let 2 = rand::random_range(1..=2) {
                break;
            }
            values.push(self.gen_expr()?);
        }
        Ok(Stmt::Local {
            names,
            values
        })
    }

    fn gen_stmt(&mut self) -> Result<Stmt, ()> {
        let chosen_stmt: u8 = rand::random_range(1..=1);
        match chosen_stmt {
            1 => self.gen_local(),
            _ => self.gen_local()
            // 2 => self.gen_assign(),
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
