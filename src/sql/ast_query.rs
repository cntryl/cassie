use serde::{Deserialize, Serialize};

use super::IdentifierPath;
use super::{ParsedStatement, TruthValue};
use crate::types::DataType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonTableExpression {
    pub name: String,
    pub aliases: Vec<String>,
    pub query: CteQuery,
    #[serde(default, skip_serializing_if = "is_default")]
    pub materialization: CteMaterialization,
}

/// Explicit CTE planning policy; recursive and volatile inputs retain sharing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CteMaterialization {
    #[default]
    Default,
    Materialized,
    NotMaterialized,
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
    IntersectAll,
    ExceptAll,
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

/// Requested cast identity when a legacy carrier is shared by SQL families.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastIdentity {
    #[default]
    Scalar,
    /// NUMERIC/DECIMAL has no qualified exact carrier in the current runtime.
    Numeric,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Column(String),
    Param(usize),
    StringLiteral(String),
    NumberLiteral(f64),
    IntegerLiteral(i64),
    /// Exact normalized decimal integer outside the existing signed carrier.
    /// Materializes only through an explicitly supported primitive cast.
    NumericLiteral(String),
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
    TruthTest {
        expr: Box<Expr>,
        truth: TruthValue,
        negated: bool,
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
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
        escape: Box<Expr>,
        /// Match literal pattern characters using Unicode simple lowercase mappings.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        case_insensitive: bool,
    },
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
        negated: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        symmetric: bool,
    },
    Not {
        expr: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        data_type: DataType,
        /// Bound source identity required by the selected conversion.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_type: Option<DataType>,
        #[serde(default)]
        identity: CastIdentity,
    },
    /// Numeric prefix operator, resolved against its operand during binding.
    UnaryNumeric {
        expr: Box<Expr>,
        op: UnaryNumericOp,
    },
    /// Integer negation applied after its operand's casts.
    IntegerNegate {
        expr: Box<Expr>,
        width: crate::types::IntegerWidth,
    },
    /// A predicate comparing corresponding row-constructor fields.
    RowCompare(Box<super::RowComparison>),
    Exists(Box<ParsedStatement>),
    /// A scalar, array, row or quantified (`IN`/`ANY`/`ALL`) subquery.
    Subquery(Box<super::SubqueryExpr>),
    Function(Box<FunctionCall>),
}

impl Expr {
    /// Returns whether this node runs a subquery (`EXISTS` or any sublink).
    #[must_use]
    pub const fn is_subquery_node(&self) -> bool {
        matches!(self, Self::Exists(_) | Self::Subquery(_))
    }

    /// Returns whether this expression or a descendant runs a subquery.
    #[must_use]
    pub fn contains_subquery(&self) -> bool {
        self.any_descendant_or_self(&mut Self::is_subquery_node)
    }

    /// The statement a subquery node runs.
    #[must_use]
    pub fn subquery_statement(&self) -> Option<&ParsedStatement> {
        match self {
            Self::Exists(statement) => Some(statement),
            Self::Subquery(subquery) => Some(&subquery.query),
            _ => None,
        }
    }

    /// The mutable statement a subquery node runs.
    pub fn subquery_statement_mut(&mut self) -> Option<&mut ParsedStatement> {
        match self {
            Self::Exists(statement) => Some(statement),
            Self::Subquery(subquery) => Some(&mut subquery.query),
            _ => None,
        }
    }
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
    /// Strict selected text concatenation.
    Concat,
    /// Strict BYTEA concatenation selected before its shared string carrier is evaluated.
    ByteaConcat,
    Add,
    Sub,
    Mul,
    Div,
    Bitwise {
        operation: crate::types::IntegerOperation,
    },
    Modulo,
    Power,
    /// A selected builtin integer operator, retaining its operand/result domain.
    Integer {
        operation: crate::types::IntegerOperation,
        width: crate::types::IntegerWidth,
    },
    Like,
    PgvectorCosine,
    PgvectorL2,
    PgvectorDot,
    /// A family operator with no scalar meaning (`<<=`, `>>=`, `&&`).
    Typed {
        operator: crate::types::extended::TypedOperator,
    },
    /// `left op ANY|SOME|ALL (array)`: `comparison` applied to every element.
    Quantified {
        comparison: Box<BinaryOp>,
        all: bool,
    },
}

impl BinaryOp {
    /// The family operator this operator names, if it has one.
    #[must_use]
    pub const fn typed_operator(&self) -> Option<crate::types::extended::TypedOperator> {
        use crate::types::extended::TypedOperator;
        use crate::types::IntegerOperation;
        let operation = match self {
            Self::Typed { operator } => return Some(*operator),
            Self::Add => return Some(TypedOperator::Add),
            Self::Sub => return Some(TypedOperator::Sub),
            Self::Concat | Self::ByteaConcat => return Some(TypedOperator::Concat),
            Self::Bitwise { operation } | Self::Integer { operation, .. } => *operation,
            _ => return None,
        };
        match operation {
            IntegerOperation::Add => Some(TypedOperator::Add),
            IntegerOperation::Sub => Some(TypedOperator::Sub),
            IntegerOperation::And => Some(TypedOperator::And),
            IntegerOperation::Or => Some(TypedOperator::Or),
            IntegerOperation::Xor => Some(TypedOperator::Xor),
            IntegerOperation::Shl => Some(TypedOperator::ShiftLeft),
            IntegerOperation::Shr => Some(TypedOperator::ShiftRight),
            IntegerOperation::Mul | IntegerOperation::Div | IntegerOperation::Rem => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bm25Params {
    pub k1: f64,
    pub b: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryNumericOp {
    Complement,
    Absolute,
    SquareRoot,
    CubeRoot,
    Plus,
    Minus,
}
