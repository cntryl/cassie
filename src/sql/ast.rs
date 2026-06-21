use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::{FieldConstraint, IndexKind};
use crate::types::DataType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedStatement {
    pub raw_sql: String,
    pub statement: QueryStatement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionArg {
    pub name: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Volatility {
    Immutable,
    Stable,
    Volatile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonTableExpression {
    pub name: String,
    pub aliases: Vec<String>,
    pub query: CteQuery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CteQuery {
    Simple(Box<ParsedStatement>),
    Recursive {
        base: Box<ParsedStatement>,
        recursive: Box<ParsedStatement>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuerySource {
    Collection(String),
    Cte(String),
    Subquery {
        alias: String,
        select: Box<SelectStatement>,
        lateral: bool,
    },
    Join {
        left: Box<QuerySource>,
        right: Box<QuerySource>,
        kind: JoinKind,
        on: Expr,
    },
    SingleRow,
}

impl PartialEq for QuerySource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Collection(left), Self::Collection(right)) => left == right,
            (Self::Cte(left), Self::Cte(right)) => left == right,
            (Self::SingleRow, Self::SingleRow) => true,
            _ => false,
        }
    }
}

impl Eq for QuerySource {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOperator {
    Union,
    UnionAll,
    Intersect,
    Except,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectSet {
    pub operator: SetOperator,
    pub right: Box<SelectStatement>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryStatement {
    Explain(ExplainStatement),
    Select(SelectStatement),
    Show(ShowStatement),
    Set(SetStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
    Transaction(TransactionStatement),
    CreateTable(CreateTableStatement),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement),
    CreateSchema(CreateSchemaStatement),
    CreateView(CreateViewStatement),
    DropView(DropViewStatement),
    CreateRole(CreateRoleStatement),
    AlterRole(AlterRoleStatement),
    DropRole(DropRoleStatement),
    CreateIndex(CreateIndexStatement),
    DropIndex(DropIndexStatement),
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CallProcedure(CallProcedureStatement),
    DropSchema(DropSchemaStatement),
    AlterSchema(AlterSchemaStatement),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainStatement {
    pub analyze: bool,
    pub statement: Box<ParsedStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionStatement {
    pub action: TransactionAction,
    pub isolation: Option<TransactionIsolation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionAction {
    Begin,
    Commit,
    Rollback,
    Savepoint { name: String },
    RollbackTo { name: String },
    Release { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionIsolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InsertSource {
    Values(Vec<Vec<Expr>>),
    Select(Box<SelectStatement>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertStatement {
    pub table: String,
    pub columns: Vec<String>,
    pub source: InsertSource,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatement {
    pub table: String,
    pub assignments: Vec<(String, Expr)>,
    pub filter: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteStatement {
    pub table: String,
    pub filter: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTableStatement {
    pub table: String,
    pub fields: Vec<FieldDefinition>,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateIndexStatement {
    pub name: String,
    pub table: String,
    pub fields: Vec<String>,
    pub expressions: Vec<Expr>,
    pub include_fields: Vec<String>,
    pub predicate: Option<Expr>,
    pub if_not_exists: bool,
    pub unique: bool,
    pub kind: IndexKind,
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFunctionStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub args: Vec<FunctionArg>,
    pub return_type: DataType,
    pub volatility: Volatility,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropFunctionStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProcedureStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub args: Vec<FunctionArg>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropProcedureStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallProcedureStatement {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowStatement {
    pub variable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetStatement {
    pub variable: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropIndexStatement {
    pub name: String,
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropTableStatement {
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterTableStatement {
    pub table: String,
    pub operation: AlterTableOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlterTableOperation {
    AddColumn { field: String, data_type: DataType },
    DropColumn { field: String },
    RenameColumn { from: String, to: String },
    RenameTo { table: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSchemaStatement {
    pub schema: String,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropSchemaStatement {
    pub schema: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterSchemaStatement {
    pub schema: String,
    pub operation: AlterSchemaOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlterSchemaOperation {
    RenameTo { schema: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateViewStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropViewStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoleStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub login: bool,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterRoleStatement {
    pub name: String,
    pub login: Option<bool>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropRoleStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<FieldConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectStatement {
    pub source: QuerySource,
    pub ctes: Vec<CommonTableExpression>,
    pub recursive: bool,
    pub distinct: bool,
    pub distinct_on: Vec<Expr>,
    pub projection: Vec<SelectItem>,
    pub filter: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub having: Option<Expr>,
    pub order: Vec<OrderExpr>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub set: Option<Box<SelectSet>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Expr {
        expr: Expr,
        alias: Option<String>,
    },
    WindowFunction {
        function: WindowFunctionCall,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFunctionCall {
    pub name: String,
    pub args: Vec<Expr>,
    pub partition_by: Vec<Expr>,
    pub order_by: Vec<OrderExpr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderExpr {
    pub expr: Expr,
    pub direction: SortDirection,
    pub nulls: Option<NullsOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullsOrder {
    First,
    Last,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Not {
        expr: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        data_type: DataType,
    },
    Exists(Box<ParsedStatement>),
    Function(FunctionCall),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bm25Params {
    pub k1: f64,
    pub b: f64,
}
