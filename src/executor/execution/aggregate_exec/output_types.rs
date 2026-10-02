//! Retain declared ARRAY types on aggregate and group columns for subsequent
//! HAVING, window and ORDER BY evaluation; grouping equality stays unchanged.
use std::collections::HashMap;

use crate::executor::array_order;
use crate::executor::batch::RowAccess;
use crate::types::DataType;

use super::{aggregate_specs, group_expr_name, AggregateExecutionContext, Batch, Expr};

pub(super) fn infer(
    batches: &[Batch],
    context: &AggregateExecutionContext<'_>,
) -> HashMap<String, DataType> {
    let Some(row) = batches.iter().flatten().next() else {
        return HashMap::new();
    };
    if !row.has_array_types() {
        return HashMap::new();
    }
    let mut types = HashMap::new();
    for expr in &context.plan.group_by {
        if let Some(data_type @ DataType::Array(_)) =
            array_order::expression_type(row, expr, context.user_functions)
        {
            types.insert(group_expr_name(expr), data_type);
        }
    }
    for spec in aggregate_specs(context.plan) {
        if let Some(data_type @ DataType::Array(_)) = array_order::expression_type(
            row,
            &Expr::Function(spec.function),
            context.user_functions,
        ) {
            for name in spec.output_names {
                types.insert(name, data_type.clone());
            }
        }
    }
    types
}

pub(super) fn attach(batches: &mut [Batch], types: &HashMap<String, DataType>) {
    if types.is_empty() {
        return;
    }
    let Some(row) = batches.iter().flatten().next() else {
        return;
    };
    let positional: std::sync::Arc<Vec<DataType>> = row
        .entries()
        .iter()
        .map(|(name, _)| types.get(name).cloned().unwrap_or(DataType::Null))
        .collect::<Vec<_>>()
        .into();
    for row in batches.iter_mut().flatten() {
        row.set_data_types(positional.clone());
    }
}
