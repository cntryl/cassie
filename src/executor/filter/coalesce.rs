//! Promote only the selected COALESCE integer carrier in its bound FLOAT domain.
use super::{FunctionCall, Value};

pub(super) fn normalize_selected(value: Value, function: &FunctionCall) -> Value {
    match value {
        Value::Int64(integer) if crate::sql::binder::coalesce_has_float_domain(function) => {
            Value::Float64(crate::types::numeric::i64_to_f64(integer))
        }
        // Existing FLOAT carriers preserve nonfinite values and signed zero.
        // Explicit user casts have already run and retain their error behavior.
        value => value,
    }
}
