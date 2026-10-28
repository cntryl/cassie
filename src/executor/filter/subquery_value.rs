//! Evaluates an executed sublink against the current row, following
//! PostgreSQL's cardinality and three-valued comparison rules.
use super::{
    binary_scalar, eval_scalar_with_context, scalar_from_value, EvalContext, PredicateResult,
    QueryError, RowAccess, ScalarValue,
};
use crate::sql::ast::{BinaryOp, Expr, RowComparisonOp, SubqueryExpr, SubqueryKind};
use crate::sql::ast::{SubqueryOutcome, SubqueryRows};
use crate::types::Value;

/// The rich value of a sublink, keeping the subquery's own carrier.
pub(super) fn value<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    subquery: &SubqueryExpr,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let resolved;
    let subquery = if subquery.outcome.is_some() {
        subquery
    } else {
        resolved = resolve(expr, context)?;
        match &resolved {
            Expr::Subquery(subquery) => subquery,
            _ => return Err(unresolved()),
        }
    };
    let retained = rows(subquery)?;
    let rows = &retained.rows;
    match &subquery.kind {
        SubqueryKind::Scalar => {
            if rows.len() > 1 {
                return Err(multiple_rows());
            }
            Ok(rows
                .first()
                .and_then(|fields| fields.first())
                .cloned()
                .unwrap_or(Value::Null))
        }
        SubqueryKind::Array => array(rows),
        SubqueryKind::Row { left, op } => {
            if rows.len() > 1 {
                return Err(multiple_rows());
            }
            let Some(fields) = rows.first() else {
                return Ok(Value::Null);
            };
            let left = operands(row, left, context)?;
            Ok(row_compare(&left, *op, fields)?.into_scalar().to_value())
        }
        SubqueryKind::Quantified {
            left,
            op,
            all,
            negated,
            ..
        } => {
            let left = operands(row, left, context)?;
            let result = quantified(&left, op, *all, rows, context)?;
            Ok(if *negated { result.not() } else { result }
                .into_scalar()
                .to_value())
        }
    }
}

pub(super) fn evaluate<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    subquery: &SubqueryExpr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    value(row, expr, subquery, context).map(|value| scalar_from_value(&value))
}

fn resolve(expr: &Expr, context: EvalContext<'_>) -> Result<Expr, QueryError> {
    let resolve = context.exists.ok_or_else(unresolved)?;
    let resolved = resolve(expr)?;
    match &resolved {
        Expr::Subquery(subquery) if subquery.outcome.is_some() => Ok(resolved),
        _ => Err(unresolved()),
    }
}

fn rows(subquery: &SubqueryExpr) -> Result<std::sync::Arc<SubqueryRows>, QueryError> {
    match &subquery.outcome {
        Some(SubqueryOutcome::Rows(rows)) => Ok(std::sync::Arc::clone(rows)),
        Some(SubqueryOutcome::Failed(error)) => Err(error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
            .map_or_else(
                || QueryError::General("subquery execution failed".into()),
                QueryError::Cassie,
            )),
        None => Err(unresolved()),
    }
}

fn operands<R: RowAccess + ?Sized>(
    row: &R,
    left: &[Expr],
    context: EvalContext<'_>,
) -> Result<Vec<ScalarValue>, QueryError> {
    left.iter()
        .map(|operand| eval_scalar_with_context(row, operand, context))
        .collect()
}

/// `left op ANY|ALL (rows)`: ANY is true when one row compares true, ALL is
/// false when one row compares false, and an undecided NULL comparison
/// otherwise leaves the result unknown. The empty set yields false for ANY
/// and true for ALL, whatever the left operand.
fn quantified(
    left: &[ScalarValue],
    op: &BinaryOp,
    all: bool,
    rows: &[Vec<Value>],
    context: EvalContext<'_>,
) -> Result<PredicateResult, QueryError> {
    let mut result = if all {
        PredicateResult::True
    } else {
        PredicateResult::False
    };
    for fields in rows {
        if let Some(controls) = context.controls {
            controls.check_interruption()?;
        }
        let compared = if left.len() == 1 {
            let right = fields.first().map_or(ScalarValue::Null, scalar_from_value);
            binary_scalar(&left[0], op, &right)?.predicate_result()?
        } else {
            let op = RowComparisonOp::from_binary(op).ok_or_else(|| {
                crate::app::CassieError::Unsupported(
                    "operator on row constructors is not yet supported".into(),
                )
            })?;
            row_compare(left, op, fields)?
        };
        match (all, compared) {
            (false, PredicateResult::True) => return Ok(PredicateResult::True),
            (true, PredicateResult::False) => return Ok(PredicateResult::False),
            (_, PredicateResult::Unknown) => result = PredicateResult::Unknown,
            _ => {}
        }
    }
    Ok(result)
}

/// Row comparison over executed fields: equality ANDs and inequality ORs
/// every field pair; an ordering comparison is decided by the first pair
/// that is not equal.
fn row_compare(
    left: &[ScalarValue],
    op: RowComparisonOp,
    right: &[Value],
) -> Result<PredicateResult, QueryError> {
    let right = right.iter().map(scalar_from_value).collect::<Vec<_>>();
    let equality = matches!(op, RowComparisonOp::Eq | RowComparisonOp::NotEq) || op.null_safe();
    let mut unknown = false;
    for (left, right) in left.iter().zip(&right) {
        let equal = binary_scalar(left, &equality_op(op), right)?.predicate_result()?;
        let equal = if op.null_safe() {
            // `IS [NOT] DISTINCT FROM` answers whether the pair differs.
            equal.not()
        } else {
            equal
        };
        match equal {
            PredicateResult::True => {}
            PredicateResult::Unknown if equality => unknown = true,
            PredicateResult::Unknown => return Ok(PredicateResult::Unknown),
            PredicateResult::False if equality => {
                return Ok(bool_result(matches!(
                    op,
                    RowComparisonOp::NotEq | RowComparisonOp::IsDistinctFrom
                )))
            }
            PredicateResult::False => {
                let strict = if matches!(op, RowComparisonOp::Lt | RowComparisonOp::Lte) {
                    BinaryOp::Lt
                } else {
                    BinaryOp::Gt
                };
                return binary_scalar(left, &strict, right)?.predicate_result();
            }
        }
    }
    if unknown {
        return Ok(PredicateResult::Unknown);
    }
    Ok(bool_result(matches!(
        op,
        RowComparisonOp::Eq
            | RowComparisonOp::Lte
            | RowComparisonOp::Gte
            | RowComparisonOp::IsNotDistinctFrom
    )))
}

fn equality_op(op: RowComparisonOp) -> BinaryOp {
    if op.null_safe() {
        BinaryOp::IsDistinctFrom
    } else {
        BinaryOp::Eq
    }
}

const fn bool_result(value: bool) -> PredicateResult {
    if value {
        PredicateResult::True
    } else {
        PredicateResult::False
    }
}

/// `ARRAY(SELECT ...)` retains SQL nulls, typed values, and nested dimensions.
fn array(rows: &[Vec<Value>]) -> Result<Value, QueryError> {
    let elements = rows
        .iter()
        .map(|fields| fields.first().cloned().unwrap_or(Value::Null))
        .collect();
    crate::types::SqlArray::from_nested_values(elements)
        .map(Value::Array)
        .map_err(|_| {
            crate::app::CassieError::ArrayInput(
                "multidimensional arrays must have array expressions with matching dimensions"
                    .to_string(),
            )
            .into()
        })
}

fn multiple_rows() -> QueryError {
    crate::app::CassieError::CardinalityViolation(
        crate::executor::execution::SUBQUERY_MULTIPLE_ROWS.into(),
    )
    .into()
}

fn unresolved() -> QueryError {
    QueryError::General("subquery was not resolved before evaluation".into())
}
