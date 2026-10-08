//! Eager conditional comparisons over the selected existing scalar families.
use super::{EvalContext, Expr, QueryError, Value};
use crate::executor::batch::RowAccess;
use std::cmp::Ordering;

/// The binder owns an outer FLOAT cast on every numeric conditional operand.
/// Normalize that carrier without changing inner, explicitly requested casts.
pub(super) fn evaluate_argument<R: RowAccess + ?Sized>(
    name: &str,
    argument: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Option<Result<Value, QueryError>> {
    if !matches!(name, "nullif" | "greatest" | "least") {
        return None;
    }
    let expr = crate::sql::binder::generated_float_coercion_operand(argument)?;
    Some(
        super::evaluate_expr_value_with_context(row, expr, context).and_then(|value| match value {
            Value::Int64(_) | Value::Float64(_) | Value::Null => Ok(promote(&value, true)),
            _ => Err(QueryError::General(
                "invalid conditional FLOAT carrier".into(),
            )),
        }),
    )
}

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    match name {
        "nullif" => Some(null_if(args)),
        "greatest" | "least" => Some(extreme(name, args)),
        _ => None,
    }
}

fn null_if(args: &[Value]) -> Result<Value, QueryError> {
    let [left, right] = args else {
        return Err(QueryError::General("nullif requires exactly 2 args".into()));
    };
    let float = args.iter().any(|value| matches!(value, Value::Float64(_)));
    if matches!(left, Value::Null)
        || (!matches!(right, Value::Null) && compare(left, right, float)?.is_eq())
    {
        Ok(Value::Null)
    } else {
        Ok(promote(left, float))
    }
}

fn extreme(name: &str, args: &[Value]) -> Result<Value, QueryError> {
    if args.is_empty() {
        return Err(QueryError::General(format!(
            "{name} requires at least 1 arg"
        )));
    }
    let float = args.iter().any(|value| matches!(value, Value::Float64(_)));
    let mut winner = None;
    for value in args.iter().filter(|value| !matches!(value, Value::Null)) {
        if let Some(previous) = winner {
            let ordering = compare(value, previous, float)?;
            if (name == "greatest" && ordering.is_gt()) || (name == "least" && ordering.is_lt()) {
                winner = Some(value);
            }
        } else {
            winner = Some(value);
        }
    }
    Ok(winner.map_or(Value::Null, |value| promote(value, float)))
}

fn promote(value: &Value, float: bool) -> Value {
    if let Value::Int64(integer) = value {
        if float {
            return Value::Float64(crate::types::numeric::i64_to_f64(*integer));
        }
    }
    value.clone()
}

fn compare(left: &Value, right: &Value, float: bool) -> Result<Ordering, QueryError> {
    let numeric = if float {
        super::compare_numeric_values(&promote(left, true), &promote(right, true))
    } else {
        super::compare_numeric_values(left, right)
    };
    if let Some(ordering) = numeric {
        return Ok(ordering);
    }
    match (left, right) {
        (Value::Bool(left), Value::Bool(right)) => Ok(left.cmp(right)),
        (Value::String(left), Value::String(right)) => Ok(super::compare_text(left, right)),
        _ => Err(QueryError::General(
            "incompatible conditional argument types".into(),
        )),
    }
}
