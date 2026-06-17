#[derive(Debug, Clone)]
pub struct ParsedStatement {
    pub raw_sql: String,
    pub statement: QueryStatement,
}

#[derive(Debug, Clone)]
pub enum QueryStatement {
    Select(SelectStatement),
}

#[derive(Debug, Clone)]
pub struct SelectStatement {
    pub collection: String,
    pub projection: Vec<SelectItem>,
    pub filter: Option<Expr>,
    pub order: Vec<OrderExpr>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum SelectItem {
    Wildcard,
    Column {
        name: String,
        alias: Option<String>,
    },
    Function {
        function: FunctionCall,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct OrderExpr {
    pub expr: Expr,
    pub direction: SortDirection,
}

#[derive(Debug, Clone)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Column(String),
    Param(usize),
    StringLiteral(String),
    NumberLiteral(f64),
    BoolLiteral(bool),
    Null,
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Function(FunctionCall),
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Like,
    PgvectorCosine,
    PgvectorL2,
    PgvectorDot,
}

#[derive(Debug, Clone)]
pub struct Bm25Params {
    pub k1: f64,
    pub b: f64,
}
