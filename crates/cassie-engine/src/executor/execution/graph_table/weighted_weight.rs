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

/// A present, finite, non-negative exact number validated without allocating.
pub(super) enum CheckedWeight<'a> {
    Int64(i64),
    Numeric(&'a Numeric),
}

pub(super) fn check_weight(value: Option<&Value>) -> Result<CheckedWeight<'_>, QueryError> {
    match value {
        Some(Value::Int64(number)) if *number >= 0 => Ok(CheckedWeight::Int64(*number)),
        Some(Value::Typed(TypedValue::Numeric(numeric)))
            if matches!(numeric, Numeric::Finite { .. })
                && numeric.signum().is_some_and(|sign| sign >= 0) =>
        {
            Ok(CheckedWeight::Numeric(numeric))
        }
        _ => Err(invalid_weight()),
    }
}

/// Materializes an exact weight for the edges retained by the search.
pub(super) fn weight(value: Option<&Value>) -> Result<Numeric, QueryError> {
    Ok(match check_weight(value)? {
        CheckedWeight::Int64(number) => Numeric::from_i64(number),
        CheckedWeight::Numeric(numeric) => numeric.clone(),
    })
}

#[cfg(test)]
#[path = "weighted_weight_tests.rs"]
mod tests;
