use serde::{Deserialize, Serialize};

use super::IdentifierPath;
use super::ParsedStatement;
use crate::types::DataType;

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
        operator: SetOperator,
        base: Box<ParsedStatement>,
        recursive: Box<ParsedStatement>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuerySource {
    Values {
        rows: Vec<Vec<Expr>>,
    },
    Collection(IdentifierPath),
    Cte(String),
    Aliased {
        source: Box<QuerySource>,
        alias: String,
        column_aliases: Vec<String>,
    },
    TableFunction {
        name: String,
        function: FunctionCall,
        lateral: bool,
    },
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
            (Self::Values { rows: left }, Self::Values { rows: right }) => {
                left.len() == right.len()
                    && left.iter().zip(right).all(|(left, right)| {
                        left.len() == right.len()
                            && left
                                .iter()
                                .zip(right)
                                .all(|(left, right)| left.structurally_eq(right))
                    })
            }
            (Self::Collection(left), Self::Collection(right)) => left == right,
            (Self::Cte(left), Self::Cte(right)) => left == right,
            (
                Self::Aliased {
                    source: left,
                    alias: left_alias,
                    column_aliases: left_columns,
                },
                Self::Aliased {
                    source: right,
                    alias: right_alias,
                    column_aliases: right_columns,
                },
            ) => left == right && left_alias == right_alias && left_columns == right_columns,
            (Self::TableFunction { name: left, .. }, Self::TableFunction { name: right, .. }) => {
                left == right
            }
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
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub args: Vec<Expr>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub quantifier: AggregateQuantifier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Box<Expr>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<OrderExpr>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregateQuantifier {
    #[default]
    Default,
    All,
    Distinct,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

impl FunctionCall {
    #[must_use]
    pub fn has_modifiers(&self) -> bool {
        self.quantifier != AggregateQuantifier::Default
            || self.filter.is_some()
            || !self.order_by.is_empty()
    }

    #[must_use]
    pub fn is_distinct(&self) -> bool {
        self.quantifier == AggregateQuantifier::Distinct
    }

    pub fn expressions(&self) -> impl Iterator<Item = &Expr> {
        self.args
            .iter()
            .chain(self.filter.as_deref())
            .chain(self.order_by.iter().map(|order| &order.expr))
    }

    pub fn expressions_mut(&mut self) -> impl Iterator<Item = &mut Expr> {
        self.args
            .iter_mut()
            .chain(self.filter.as_deref_mut())
            .chain(self.order_by.iter_mut().map(|order| &mut order.expr))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFunctionCall {
    pub name: String,
    pub args: Vec<Expr>,
    pub partition_by: Vec<Expr>,
    pub order_by: Vec<OrderExpr>,
    pub frame: Option<WindowFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub unit: WindowFrameUnit,
    pub start: WindowFrameBound,
    pub end: WindowFrameBound,
    #[serde(default)]
    pub exclusion: WindowFrameExclusion,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowFrameExclusion {
    #[default]
    NoOthers,
    CurrentRow,
    Group,
    Ties,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowFrameUnit {
    Rows,
    Range,
    Groups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowFrameBound {
    UnboundedPreceding,
    Preceding(u64),
    CurrentRow,
    Following(u64),
    UnboundedFollowing,
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
    IntegerLiteral(i64),
    BoolLiteral(bool),
    Null,
    Case {
        operand: Option<Box<Expr>>,
        branches: Vec<(Expr, Expr)>,
        else_expr: Option<Box<Expr>>,
    },
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
    /// Integer negation applied after its operand's casts.
    IntegerNegate {
        expr: Box<Expr>,
        width: crate::types::IntegerWidth,
    },
    Exists(Box<ParsedStatement>),
    Function(Box<FunctionCall>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Eq,
    NotEq,
    IsDistinctFrom,
    IsNotDistinctFrom,
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
    /// A selected builtin integer operator, retaining its operand/result domain.
    Integer {
        operation: crate::types::IntegerOperation,
        width: crate::types::IntegerWidth,
    },
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
