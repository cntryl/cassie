//! Numeric comparison for simple CHECK constraints.
//!
//! The stored document is JSON, where `1` and `1.0` are distinct values, but
//! SQL compares numbers by value. Integers compare exactly (so BIGINT values
//! beyond 2^53 stay distinct); only a mixed integer/float pair widens to
//! `f64`, as PostgreSQL does after its implicit cast to `float8`.

use crate::catalog::{CollectionSchema, ConstraintCheck, ConstraintOperator};
use crate::types::extended::{ExtendedFamily, ExtendedType};
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

/// Applies a comparison operator to the order of the stored value against
/// the bound, or `None` for an operator that does not order values.
fn satisfies(order: Ordering, operator: &ConstraintOperator) -> Option<bool> {
    Some(match operator {
        ConstraintOperator::Eq | ConstraintOperator::IsNotDistinctFrom => order.is_eq(),
        ConstraintOperator::NotEq | ConstraintOperator::IsDistinctFrom => order.is_ne(),
        ConstraintOperator::Lt => order.is_lt(),
        ConstraintOperator::Lte => order.is_le(),
        ConstraintOperator::Gt => order.is_gt(),
        ConstraintOperator::Gte => order.is_ge(),
        ConstraintOperator::Like => return None,
    })
}

/// Evaluates a comparison CHECK on a column whose values compare as typed
/// values, not as their JSON text: `NUMERIC` by decimal value, `float4`,
/// temporal and interval types by instant or span, and `float8` where a bound
/// or value is `NaN` or an infinity. `None` when the column is none of those
/// or the check is not a comparison.
pub(super) fn typed_column_check(
    schema: Option<&CollectionSchema>,
    value: &serde_json::Value,
    check: &ConstraintCheck,
) -> Option<bool> {
    let column = schema?
        .fields
        .iter()
        .find(|candidate| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(&check.field, &candidate.name)
        })?
        .data_type
        .flattened();
    if check.bound_expression.is_some() || matches!(check.operator, ConstraintOperator::Like) {
        return None;
    }
    match column {
        DataType::Float => float_check(value, check),
        DataType::Extended(kind) => extended_check(*kind, value, check),
        _ => None,
    }
}

fn extended_check(
    kind: ExtendedType,
    value: &serde_json::Value,
    check: &ConstraintCheck,
) -> Option<bool> {
    if !matches!(
        kind.family(),
        ExtendedFamily::Numeric | ExtendedFamily::Temporal | ExtendedFamily::Timespan
    ) {
        return None;
    }
    let left = kind.from_json(value).ok()?;
    let right = kind.from_json(&check.value).ok()?;
    let order = match (&left, &right) {
        (TypedValue::Numeric(left), TypedValue::Numeric(right)) => left.compare(right),
        _ => left.key().cmp(&right.key()),
    };
    satisfies(order, &check.operator)
}

/// `float8` values are JSON numbers, except `NaN` and the infinities, which
/// JSON cannot carry and which are stored as their text. PostgreSQL orders
/// `NaN` above every number and equal to itself.
fn float_check(value: &serde_json::Value, check: &ConstraintCheck) -> Option<bool> {
    if !value.is_string() && !check.value.is_string() {
        return None;
    }
    let left = float_of(value)?;
    let right = float_of(&check.value)?;
    let order = match (left.is_nan(), right.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => left.partial_cmp(&right)?,
    };
    satisfies(order, &check.operator)
}

fn float_of(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => crate::types::float_input::parse(text).ok(),
        _ => None,
    }
}
