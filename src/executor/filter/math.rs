//! Existing FLOAT8 math, shared by selected PostgreSQL operators and calls.
use super::{QueryError, Value};
use crate::app::CassieError;

pub(super) const POWER_ZERO: &str = "zero raised to a negative power is undefined";
pub(super) const POWER_COMPLEX: &str =
    "a negative number raised to a non-integer power yields a complex result";

pub(in crate::executor::filter) fn evaluate(
    name: &str,
    args: &[Value],
) -> Option<Result<Value, QueryError>> {
    (name == "power").then(|| {
        let [left, right] = args else {
            return Err(QueryError::General(
                "function power(...) does not exist".into(),
            ));
        };
        if matches!(left, Value::Null) || matches!(right, Value::Null) {
            return Ok(Value::Null);
        }
        let left = number(left)?;
        let right = number(right)?;
        power(left, right).map(Value::Float64)
    })
}
pub(in crate::executor::filter) fn power(left: f64, right: f64) -> Result<f64, QueryError> {
    if left == 0.0 && right < 0.0 {
        return Err(QueryError::General(POWER_ZERO.into()));
    }
    if left < 0.0 && right.is_finite() && right.fract() != 0.0 {
        return Err(QueryError::General(POWER_COMPLEX.into()));
    }
    let result = left.powf(right);
    if left.is_finite()
        && right.is_finite()
        && (result.is_infinite() || (result == 0.0 && left != 0.0))
    {
        return Err(CassieError::FloatInput(crate::types::FloatInputError::Range).into());
    }
    Ok(result)
}
fn number(value: &Value) -> Result<f64, QueryError> {
    match value {
        Value::Float64(value) => Ok(*value),
        Value::Int64(value) => Ok(crate::types::numeric::i64_to_f64(*value)),
        _ => Err(QueryError::General(
            "function power input requires double precision".into(),
        )),
    }
}
