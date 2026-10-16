//! Pairwise row predicates retain native NULL and evaluation order.
use super::{eval_scalar_with_context, EvalContext, QueryError, RowAccess, ScalarValue};
use crate::sql::ast::{RowComparison, RowComparisonOp};

pub(super) fn evaluate<R: RowAccess + ?Sized>(
    row: &R,
    comparison: &RowComparison,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    if comparison.left.len() != comparison.right.len() {
        return Err(crate::app::CassieError::InvalidQuery(
            "unequal number of entries in row expressions".into(),
        )
        .into());
    }
    if comparison.left.is_empty() && !comparison.op.null_safe() {
        return Err(crate::app::CassieError::Unsupported(
            "cannot compare rows of zero length".into(),
        )
        .into());
    }
    let equality = matches!(comparison.op, RowComparisonOp::Eq | RowComparisonOp::NotEq)
        || comparison.op.null_safe();
    // PostgreSQL simplifies equality/distinctness as AND/OR predicates:
    // a deciding immutable field prunes earlier variable fields as well.
    if equality {
        if let Some(value) = constant_decision(row, comparison, context)? {
            return Ok(ScalarValue::Bool(value));
        }
    }
    let mut has_null = false;
    for (left, right) in comparison.left.iter().zip(&comparison.right) {
        let left = eval_scalar_with_context(row, left, context)?;
        let right = eval_scalar_with_context(row, right, context)?;
        match compare(&left, &right, comparison.op.null_safe())? {
            Some(std::cmp::Ordering::Equal) => {}
            Some(_) if equality => {
                return Ok(ScalarValue::Bool(matches!(
                    comparison.op,
                    RowComparisonOp::NotEq | RowComparisonOp::IsDistinctFrom
                )))
            }
            Some(ordering) => {
                return Ok(ScalarValue::Bool(
                    if matches!(comparison.op, RowComparisonOp::Gt | RowComparisonOp::Gte) {
                        ordering.is_gt()
                    } else {
                        ordering.is_lt()
                    },
                ))
            }
            None if equality => has_null = true,
            None => return Ok(ScalarValue::Null),
        }
    }
    if has_null {
        return Ok(ScalarValue::Null);
    }
    Ok(ScalarValue::Bool(matches!(
        comparison.op,
        RowComparisonOp::Eq
            | RowComparisonOp::Lte
            | RowComparisonOp::Gte
            | RowComparisonOp::IsNotDistinctFrom
    )))
}

fn compare(
    left: &ScalarValue,
    right: &ScalarValue,
    null_safe: bool,
) -> Result<Option<std::cmp::Ordering>, QueryError> {
    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) if null_safe => Ok(Some(std::cmp::Ordering::Equal)),
        (ScalarValue::Null, _) if null_safe => Ok(Some(std::cmp::Ordering::Less)),
        (_, ScalarValue::Null) if null_safe => Ok(Some(std::cmp::Ordering::Greater)),
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => Ok(None),
        (ScalarValue::Str(left), ScalarValue::Str(right)) => Ok(Some(left.cmp(right))),
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => Ok(Some(left.cmp(right))),
        (
            ScalarValue::Int(_) | ScalarValue::Float(_),
            ScalarValue::Int(_) | ScalarValue::Float(_),
        ) => super::compare_numeric_values(&left.to_value(), &right.to_value())
            .map(Some)
            .ok_or_else(|| {
                crate::app::CassieError::Unsupported(
                    "non-finite row comparison is not yet supported".into(),
                )
                .into()
            }),
        _ => Err(QueryError::General(
            "operator does not exist: incompatible row fields".into(),
        )),
    }
}

pub(crate) fn field_result(
    left: &ScalarValue,
    op: RowComparisonOp,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    Ok(
        compare(left, right, op.null_safe())?.map_or(ScalarValue::Null, |ordering| {
            ScalarValue::Bool(match op {
                RowComparisonOp::Eq | RowComparisonOp::IsNotDistinctFrom => ordering.is_eq(),
                RowComparisonOp::NotEq | RowComparisonOp::IsDistinctFrom => !ordering.is_eq(),
                RowComparisonOp::Lt => ordering.is_lt(),
                RowComparisonOp::Lte => !ordering.is_gt(),
                RowComparisonOp::Gt => ordering.is_gt(),
                RowComparisonOp::Gte => !ordering.is_lt(),
            })
        }),
    )
}

fn constant_decision<R: RowAccess + ?Sized>(
    row: &R,
    comparison: &RowComparison,
    context: EvalContext<'_>,
) -> Result<Option<bool>, QueryError> {
    if !matches!(comparison.op, RowComparisonOp::Eq | RowComparisonOp::NotEq)
        && !comparison.op.null_safe()
    {
        return Ok(None);
    }
    let is_or = matches!(
        comparison.op,
        RowComparisonOp::NotEq | RowComparisonOp::IsDistinctFrom
    );
    for (left, right) in comparison.left.iter().zip(&comparison.right) {
        if let (Some(left), Some(right)) = (
            constants::value(left, row, context)?,
            constants::value(right, row, context)?,
        ) {
            if let ScalarValue::Bool(value) = field_result(&left, comparison.op, &right)? {
                if value == is_or {
                    return Ok(Some(value));
                }
            }
        }
    }
    Ok(None)
}

#[path = "row_constants.rs"]
mod constants;
