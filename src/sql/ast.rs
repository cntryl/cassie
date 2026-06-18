use std::collections::BTreeMap;

use crate::catalog::{FieldConstraint, IndexKind};
use crate::types::DataType;

#[derive(Debug, Clone)]
pub struct ParsedStatement {
    pub raw_sql: String,
    pub statement: QueryStatement,
}

#[derive(Debug, Clone)]
pub struct FunctionArg {
    pub name: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone)]
pub enum Volatility {
    Immutable,
    Stable,
    Volatile,
}

#[derive(Debug, Clone)]
pub struct CommonTableExpression {
    pub name: String,
    pub aliases: Vec<String>,
    pub query: CteQuery,
}

#[derive(Debug, Clone)]
pub enum CteQuery {
    Simple(Box<ParsedStatement>),
    Recursive {
        base: Box<ParsedStatement>,
        recursive: Box<ParsedStatement>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySource {
    Collection(String),
    Cte(String),
}

#[derive(Debug, Clone)]
pub enum QueryStatement {
    Select(SelectStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
    Transaction(TransactionStatement),
    CreateTable(CreateTableStatement),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement),
    CreateSchema(CreateSchemaStatement),
    CreateIndex(CreateIndexStatement),
    DropIndex(DropIndexStatement),
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CallProcedure(CallProcedureStatement),
}

#[derive(Debug, Clone)]
pub struct TransactionStatement {
    pub action: TransactionAction,
    pub isolation: Option<TransactionIsolation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionAction {
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionIsolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

#[derive(Debug, Clone)]
pub enum InsertSource {
    Values(Vec<Expr>),
    Select(SelectStatement),
}

#[derive(Debug, Clone)]
pub struct InsertStatement {
    pub table: String,
    pub columns: Vec<String>,
    pub source: InsertSource,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone)]
pub struct UpdateStatement {
    pub table: String,
    pub assignments: Vec<(String, Expr)>,
    pub filter: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone)]
pub struct DeleteStatement {
    pub table: String,
    pub filter: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone)]
pub struct CreateTableStatement {
    pub table: String,
    pub fields: Vec<FieldDefinition>,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone)]
pub struct CreateIndexStatement {
    pub name: String,
    pub table: String,
    pub field: String,
    pub if_not_exists: bool,
    pub unique: bool,
    pub kind: IndexKind,
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CreateFunctionStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub args: Vec<FunctionArg>,
    pub return_type: DataType,
    pub volatility: Volatility,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct DropFunctionStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub struct CreateProcedureStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub args: Vec<FunctionArg>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct DropProcedureStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub struct CallProcedureStatement {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct DropIndexStatement {
    pub name: String,
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub struct DropTableStatement {
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub struct AlterTableStatement {
    pub table: String,
    pub operation: AlterTableOperation,
}

#[derive(Debug, Clone)]
pub enum AlterTableOperation {
    AddColumn { field: String, data_type: DataType },
    DropColumn { field: String },
    RenameTo { table: String },
}

#[derive(Debug, Clone)]
pub struct CreateSchemaStatement {
    pub schema: String,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone)]
pub struct FieldDefinition {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<FieldConstraint>,
}

#[derive(Debug, Clone)]
pub struct SelectStatement {
    pub source: QuerySource,
    pub ctes: Vec<CommonTableExpression>,
    pub recursive: bool,
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
    pub nulls: Option<NullsOrder>,
}

#[derive(Debug, Clone)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullsOrder {
    First,
    Last,
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
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
    InList {
        expr: Box<Expr>,
        values: Vec<Expr>,
        negated: bool,
    },
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
        negated: bool,
    },
    Cast {
        expr: Box<Expr>,
        data_type: DataType,
    },
    Exists(Box<ParsedStatement>),
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
