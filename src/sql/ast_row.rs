//! Row-constructor predicates without a stored composite carrier.
use super::{BinaryOp, Expr};
use serde::{Deserialize, Serialize};

/// The supported SQL row-constructor predicate operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowComparisonOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    IsDistinctFrom,
    IsNotDistinctFrom,
}

/// Corresponding constructor fields, retained in SQL source order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowComparison {
    pub left: Vec<Expr>,
    pub op: RowComparisonOp,
    pub right: Vec<Expr>,
}

impl RowComparisonOp {
    pub(crate) const fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::IsDistinctFrom => "IS DISTINCT FROM",
            Self::IsNotDistinctFrom => "IS NOT DISTINCT FROM",
        }
    }

    pub(crate) fn from_binary(op: &BinaryOp) -> Option<Self> {
        Some(match op {
            BinaryOp::Eq => Self::Eq,
            BinaryOp::NotEq => Self::NotEq,
            BinaryOp::Lt => Self::Lt,
            BinaryOp::Lte => Self::Lte,
            BinaryOp::Gt => Self::Gt,
            BinaryOp::Gte => Self::Gte,
            BinaryOp::IsDistinctFrom => Self::IsDistinctFrom,
            BinaryOp::IsNotDistinctFrom => Self::IsNotDistinctFrom,
            _ => return None,
        })
    }
    pub(crate) fn binary(self) -> BinaryOp {
        match self {
            Self::Eq => BinaryOp::Eq,
            Self::NotEq => BinaryOp::NotEq,
            Self::Lt => BinaryOp::Lt,
            Self::Lte => BinaryOp::Lte,
            Self::Gt => BinaryOp::Gt,
            Self::Gte => BinaryOp::Gte,
            Self::IsDistinctFrom => BinaryOp::IsDistinctFrom,
            Self::IsNotDistinctFrom => BinaryOp::IsNotDistinctFrom,
        }
    }
    pub(crate) fn null_safe(self) -> bool {
        matches!(self, Self::IsDistinctFrom | Self::IsNotDistinctFrom)
    }
}
