//! Selected ordered-set aggregate result identities.
use super::{infer_expr_type, DataType, Expr, FunctionCall, HashMap, Schema};

pub(super) fn selected_ordered_set_return_type(
    function: &FunctionCall,
    id: crate::sql::functions::FunctionId,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    if !matches!(
        id,
        crate::sql::functions::FunctionId::Mode
            | crate::sql::functions::FunctionId::PercentileCont
            | crate::sql::functions::FunctionId::PercentileDisc
    ) {
        return None;
    }
    ordered_set_return_type(function, id, source_schema, user_functions, parameter_types)
}

fn ordered_set_return_type(
    function: &FunctionCall,
    id: crate::sql::functions::FunctionId,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let order_type = function.order_by.first().and_then(|order| {
        infer_expr_type(&order.expr, source_schema, user_functions, parameter_types)
    })?;
    if id == crate::sql::functions::FunctionId::Mode {
        return Some(order_type);
    }
    let result_element = match id {
        crate::sql::functions::FunctionId::PercentileCont
            if matches!(
                order_type,
                DataType::Extended(crate::types::extended::ExtendedType::Interval { .. })
            ) =>
        {
            order_type
        }
        crate::sql::functions::FunctionId::PercentileCont => DataType::Float,
        crate::sql::functions::FunctionId::PercentileDisc => order_type,
        _ => return None,
    };
    let fraction_is_array = function.args.first().is_some_and(|fraction| {
        matches!(
            fraction,
            Expr::Subquery(subquery)
                if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array)
        ) || infer_expr_type(fraction, source_schema, user_functions, parameter_types)
            .is_some_and(|data_type| matches!(data_type, DataType::Array(_)))
    });
    Some(if fraction_is_array {
        DataType::Array(Box::new(result_element))
    } else {
        result_element
    })
}
