pub const DEFAULT_FUEL: i32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Costs {
    // ---- expressions --------------------------------------------------

    pub literal: i32,
    pub var: i32,
    pub varargs: i32,
    pub paren: i32,
    pub unary: i32,
    pub binary: i32,
    pub field: i32,
    pub index: i32,
    pub call: i32,
    pub method_call_extra: i32,
    pub function_expr: i32,
    pub table: i32,
    pub table_field_positional: i32,
    pub table_field_named: i32,
    pub table_field_keyed: i32,
    pub if_else_expr: i32,

    // ---- function parts -----------------------------------------------

    pub param: i32,
    pub type_annot: i32,

    // ---- statements ---------------------------------------------------

    pub local: i32,
    pub extra_target: i32,
    pub assign: i32,
    pub compound_assign: i32,
    pub function_decl: i32,
    pub call_stmt: i32,
    pub do_block: i32,
    pub while_loop: i32,
    pub repeat_loop: i32,
    pub if_stmt: i32,
    pub elseif_branch: i32,
    pub else_branch: i32,
    pub numeric_for: i32,
    pub generic_for: i32,
    pub generic_for_extra_var: i32,

    // ---- last statements ----------------------------------------------

    pub return_stmt: i32,
    pub break_stmt: i32,
    pub continue_stmt: i32,

    // ---- structural limits (not fuel, but they belong with it) --------

    pub min_body_reserve: i32,
    pub max_block_depth: i32,
    pub max_expr_depth: i32,
    pub max_function_depth: i32,
}

pub const DEFAULT_COSTS: Costs = Costs {
    // expressions
    literal: 1,
    var: 1,
    varargs: 1,
    paren: 1,
    unary: 2,
    binary: 3,
    field: 2,
    index: 3,
    call: 4,
    method_call_extra: 1,
    function_expr: 12,
    table: 3,
    table_field_positional: 1,
    table_field_named: 2,
    table_field_keyed: 3,
    if_else_expr: 4,

    // function parts
    param: 1,
    type_annot: 1,

    // statements
    local: 3,
    extra_target: 1,
    assign: 3,
    compound_assign: 3,
    function_decl: 12,
    call_stmt: 4,
    do_block: 3,
    while_loop: 8,
    repeat_loop: 8,
    if_stmt: 6,
    elseif_branch: 4,
    else_branch: 2,
    numeric_for: 8,
    generic_for: 9,
    generic_for_extra_var: 1,

    // last statements
    return_stmt: 2,
    break_stmt: 1,
    continue_stmt: 1,

    // structural limits
    min_body_reserve: 8,
    max_block_depth: 5,
    max_expr_depth: 6,
    max_function_depth: 3,
};