//! Numeric comparison for simple CHECK constraints.
//!
//! The stored document is JSON, where `1` and `1.0` are distinct values, but
//! SQL compares numbers by value. Integers compare exactly (so BIGINT values
//! beyond 2^53 stay distinct); only a mixed integer/float pair widens to
//! `f64`, as PostgreSQL does after its implicit cast to `float8`.

use crate::catalog::{CollectionSchema, ConstraintCheck, ConstraintOperator};
use crate::types::extended::ExtendedType;
use crate::types::typed::TypedValue;
use crate::types::DataType;
use std::cmp::Ordering;

/// Orders two JSON numbers by value, or `None` when either is not comparable.
pub(super) fn compare_numbers(
    left: &serde_json::Number,
    right: &serde_json::Number,
) -> Option<Ordering> {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return Some(left.cmp(&right));
    }
    if let (Some(left), Some(right)) = (left.as_u64(), right.as_u64()) {
        return Some(left.cmp(&right));
    }
    left.as_f64()
        .zip(right.as_f64())
        .and_then(|(left, right)| left.partial_cmp(&right))
}

/// Whether a stored value equals a CHECK bound, comparing numbers by value.
pub(super) fn values_equal(value: &serde_json::Value, bound: &serde_json::Value) -> bool {
    match (value, bound) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            compare_numbers(left, right) == Some(Ordering::Equal)
        }
        _ => value == bound,
    }
}

/// Evaluates a comparison CHECK on a `NUMERIC` column by decimal value, or
/// `None` when the column is not `NUMERIC` or the check is not a comparison.
pub(super) fn numeric_column_check(
    schema: Option<&CollectionSchema>,
    value: &serde_json::Value,
    check: &ConstraintCheck,
) -> Option<bool> {
    let kind = schema?
        .fields
        .iter()
        .find(|candidate| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(&check.field, &candidate.name)
        })
        .and_then(|candidate| match &candidate.data_type {
            DataType::Extended(kind @ ExtendedType::Numeric(_)) => Some(*kind),
            _ => None,
        })?;
    if check.bound_expression.is_some() || matches!(check.operator, ConstraintOperator::Like) {
        return None;
    }
    let (TypedValue::Numeric(left), TypedValue::Numeric(right)) = (
        kind.from_json(value).ok()?,
        kind.from_json(&check.value).ok()?,
    ) else {
        return None;
    };
    let order = left.compare(&right);
    Some(match check.operator {
        ConstraintOperator::Eq | ConstraintOperator::IsNotDistinctFrom => order.is_eq(),
        ConstraintOperator::NotEq | ConstraintOperator::IsDistinctFrom => order.is_ne(),
        ConstraintOperator::Lt => order.is_lt(),
        ConstraintOperator::Lte => order.is_le(),
        ConstraintOperator::Gt => order.is_gt(),
        ConstraintOperator::Gte => order.is_ge(),
        ConstraintOperator::Like => return None,
    })
}
