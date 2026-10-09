//! Deferred EXISTS keys use the admitted scalar sort with actual input rows.
use super::{
    chunk_rows_controlled, sort_rows_by_key, Batch, EvalInput, Expr, QueryExecutionControls,
    SortRetentionContext, Value,
};
use crate::executor::batch::BatchRow;
use crate::executor::QueryError;

pub(crate) fn sort_batches_resolving(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
    evaluate: &dyn Fn(&BatchRow, &Expr) -> Result<Value, QueryError>,
) -> Result<Vec<Batch>, QueryError> {
    if eval.order.is_empty() {
        return Ok(batches);
    }
    let (rows, _flatten_memory) = super::typed::flatten(batches, controls)?;
    let order = eval.resolved_order();
    let retention = SortRetentionContext::default();
    let rows = sort_rows_by_key(rows, controls, &retention, |row| {
        eval.row_key_with_evaluator(row, &order, &retention, Some(controls), evaluate)
    })?;
    let output = chunk_rows_controlled(rows.into_iter(), controls)?;
    crate::executor::typed_batch::relational_diagnostics::publish(
        "sort",
        "scalar_expression_order",
    );
    Ok(output)
}

/// Inspect the same borrowed alias selection used by key construction.
pub(crate) fn requires_resolver(eval: &EvalInput<'_>) -> bool {
    let contains =
        |expr: &Expr| expr.any_descendant_or_self(&mut |child| matches!(child, Expr::Exists(_)));
    if eval.order.iter().any(|order| contains(&order.expr)) {
        return true;
    }
    let deferred_projection = eval.projection.iter().any(|item| match item {
        super::SelectItem::Expr { expr, .. } => contains(expr),
        super::SelectItem::Function { function, .. } => function.args.iter().any(contains),
        _ => false,
    });
    if !deferred_projection {
        return false;
    }
    eval.order.iter().any(
        |order| match super::alias_item(&order.expr, eval.projection) {
            Some(super::SelectItem::Expr { expr, .. }) => contains(expr),
            Some(super::SelectItem::Function { function, .. }) => {
                function.args.iter().any(contains)
            }
            _ => false,
        },
    )
}

#[cfg(test)]
#[path = "exists_tests.rs"]
mod tests;
