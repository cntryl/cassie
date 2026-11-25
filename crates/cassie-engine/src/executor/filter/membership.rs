//! `IN (...)` and `BETWEEN` predicates with SQL three-valued results.
use super::{
    binary_expr, bool_to_predicate, eq_value, eval_scalar_with_context, ordered_cmp, EvalContext,
    Expr, PredicateResult, QueryError, RowAccess, ScalarValue,
};

pub(super) fn eval_in_list_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    values: &[Expr],
    negated: bool,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let left = eval_scalar_with_context(row, expr, context)?;
    let mut result = PredicateResult::False;
    for value in values {
        let equal = match super::composite_cast::constructor_equality(row, expr, value, context)? {
            Some(ScalarValue::Bool(equal)) => Some(equal),
            Some(_) => None,
            None => eq_value(&left, &eval_scalar_with_context(row, value, context)?),
        };
        result = match equal {
            Some(true) => PredicateResult::True,
            None if matches!(result, PredicateResult::False) => PredicateResult::Unknown,
            Some(false) | None => result,
        };
        if result.is_true() {
            break;
        }
    }
    if negated {
        Ok(result.not().into_scalar())
    } else {
        Ok(result.into_scalar())
    }
}

pub(super) fn eval_between_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    negated: bool,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let value = eval_scalar_with_context(row, expr, context)?;
    let low_value = eval_scalar_with_context(row, low, context)?;
    let high_value = eval_scalar_with_context(row, high, context)?;
    let in_range =
        binary_expr::compare_array_operands(row, (expr, &value), (low, &low_value), context)?
            .map(|ordering| !ordering.is_lt())
            .or_else(|| ordered_cmp(&value, &low_value, |ordering| !ordering.is_lt()))
            .map_or(PredicateResult::Unknown, bool_to_predicate)
            .and(
                binary_expr::compare_array_operands(
                    row,
                    (expr, &value),
                    (high, &high_value),
                    context,
                )?
                .map(|ordering| !ordering.is_gt())
                .or_else(|| ordered_cmp(&value, &high_value, |ordering| !ordering.is_gt()))
                .map_or(PredicateResult::Unknown, bool_to_predicate),
            );
    if negated {
        Ok(in_range.not().into_scalar())
    } else {
        Ok(in_range.into_scalar())
    }
}
