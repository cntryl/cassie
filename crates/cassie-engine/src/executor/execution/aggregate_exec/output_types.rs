//! Retain declared ARRAY types on aggregate and group columns for subsequent
//! HAVING, window and ORDER BY evaluation; grouping equality stays unchanged.
use std::collections::HashMap;

use crate::executor::array_order;
use crate::types::DataType;

use super::{aggregate_specs, group_expr_name, AggregateExecutionContext, Batch, Expr};

pub(super) fn infer(
    batches: &[Batch],
    context: &AggregateExecutionContext<'_>,
) -> HashMap<String, DataType> {
    let Some(row) = batches.iter().flatten().next() else {
        return HashMap::new();
    };
    let mut types = HashMap::new();
    for expr in &context.plan.group_by {
        if let Some(data_type @ (DataType::Array(_) | DataType::Char { .. } | DataType::Jsonb)) =
            array_order::expression_type_with_parameter_oids(
                row,
                expr,
                context.user_functions,
                context.parameter_type_oids,
            )
        {
            types.insert(group_expr_name(expr), data_type);
        }
    }
    for spec in aggregate_specs(context.plan) {
        if let Some(data_type @ (DataType::Array(_) | DataType::Char { .. } | DataType::Jsonb)) =
            array_order::expression_type_with_parameter_oids(
                row,
                &Expr::Function(Box::new(spec.function)),
                context.user_functions,
                context.parameter_type_oids,
            )
        {
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
