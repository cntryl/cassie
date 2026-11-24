//! PostgreSQL's symmetric BETWEEN rewrite, evaluated without copying operand trees.
use crate::sql::ast::{BinaryOp, Expr};

/// A comparison may be SQL NULL or may depend on a runtime value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comparison {
    Variable,
    Known(Option<bool>),
}

pub(crate) fn evaluate<E>(
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    negated: bool,
    mut compare: impl FnMut(&Expr, BinaryOp, &Expr) -> Result<Comparison, E>,
) -> Result<Comparison, E> {
    let (first, second) = if negated {
        (BinaryOp::Lt, BinaryOp::Gt)
    } else {
        (BinaryOp::Gte, BinaryOp::Lte)
    };
    let mut orientation = |low, high| {
        let left = compare(expr, first.clone(), low)?;
        if left == Comparison::Known(Some(negated)) {
            return Ok(left);
        }
        let right = compare(expr, second.clone(), high)?;
        Ok(combine(left, right, negated))
    };
    let forward = orientation(low, high)?;
    if forward == Comparison::Known(Some(!negated)) {
        return Ok(forward);
    }
    Ok(combine(forward, orientation(high, low)?, !negated))
}

fn combine(left: Comparison, right: Comparison, is_or: bool) -> Comparison {
    if left == Comparison::Known(Some(is_or)) || right == Comparison::Known(Some(is_or)) {
        return Comparison::Known(Some(is_or));
    }
    match (left, right) {
        (Comparison::Known(Some(_)), Comparison::Known(Some(_))) => Comparison::Known(Some(!is_or)),
        (Comparison::Known(_), Comparison::Known(_)) => Comparison::Known(None),
        _ => Comparison::Variable,
    }
}

pub(super) fn scalar(value: Option<bool>) -> super::ScalarValue {
    value.map_or(super::ScalarValue::Null, super::ScalarValue::Bool)
}

pub(super) fn truth(value: super::ScalarValue) -> Result<Option<bool>, super::QueryError> {
    match value {
        super::ScalarValue::Null => Ok(None),
        super::ScalarValue::Bool(value) => Ok(Some(value)),
        value => value.predicate_result().map(|result| {
            if matches!(result, super::PredicateResult::Unknown) {
                None
            } else {
                Some(result.is_true())
            }
        }),
    }
}

pub(super) fn runtime<R: super::RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    context: super::EvalContext<'_>,
) -> Result<super::ScalarValue, super::QueryError> {
    if let Some(value) = super::constants::value(expression, row, context)? {
        return Ok(value);
    }
    let Expr::Between {
        expr,
        low,
        high,
        negated,
        ..
    } = expression
    else {
        unreachable!("symmetric BETWEEN dispatch")
    };
    evaluate(expr, low, high, *negated, |left, op, right| {
        let left = super::eval_scalar_with_context(row, left, context)?;
        let right = super::eval_scalar_with_context(row, right, context)?;
        truth(super::binary_scalar(&left, &op, &right)?).map(Comparison::Known)
    })
    .map(|value| match value {
        Comparison::Known(value) => scalar(value),
        Comparison::Variable => unreachable!("runtime comparisons are resolved"),
    })
}
