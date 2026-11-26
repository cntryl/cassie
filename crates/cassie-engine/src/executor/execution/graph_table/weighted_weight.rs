//! The weight of one edge of `graph_weighted_paths` (#1258): present, finite, non-negative
//! and an exact number.

use super::weighted_args::diagnostic;
use super::QueryError;
use crate::types::extended::numeric::Numeric;
use crate::types::typed::TypedValue;
use crate::types::Value;

pub(super) fn invalid_weight() -> QueryError {
    diagnostic(
        "22023",
        "a selected edge has a missing, negative or non-finite weight",
    )
}

/// A present, finite, non-negative exact number.
pub(super) fn weight(value: Option<&Value>) -> Result<Numeric, QueryError> {
    let numeric = match value {
        Some(Value::Int64(number)) => Numeric::from_i64(*number),
        Some(Value::Typed(TypedValue::Numeric(numeric))) => numeric.clone(),
        _ => return Err(invalid_weight()),
    };
    match numeric.signum() {
        Some(sign) if sign >= 0 && matches!(numeric, Numeric::Finite { .. }) => Ok(numeric),
        _ => Err(invalid_weight()),
    }
}

#[cfg(test)]
#[path = "weighted_weight_tests.rs"]
mod tests;
