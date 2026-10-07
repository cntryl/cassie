//! The selected existing-type conditional expression contract.
use super::coalesce_results::ResultTypes;
use super::{CassieError, DataType, Expr, FunctionCall, SelectItem};

pub(super) fn is_conditional(name: &str) -> bool {
    ["nullif", "greatest", "least"]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

pub(super) fn result_type(name: &str, arguments: &[DataType]) -> Result<DataType, CassieError> {
    if (name.eq_ignore_ascii_case("nullif") && arguments.len() != 2) || arguments.is_empty() {
        return Err(CassieError::Planner(format!(
            "invalid {name} argument count"
        )));
    }
    let mut common = DataType::Null;
    for argument in arguments {
        if !matches!(
            argument,
            DataType::Null
                | DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Text
                | DataType::Boolean
        ) {
            return Err(CassieError::Unsupported(format!(
                "{name} argument type is outside the selected conditional expression contract"
            )));
        }
        common = super::inference::common_case_type(common, argument.clone())
            .ok_or_else(|| CassieError::Planner(format!("incompatible {name} argument types")))?;
    }
    let result = if name.eq_ignore_ascii_case("nullif") && common != DataType::Float {
        arguments
            .first()
            .filter(|first| **first != DataType::Null)
            .cloned()
            .unwrap_or(common)
    } else {
        common
    };
    Ok(if result == DataType::Null {
        DataType::Text
    } else {
        result
    })
}

pub(super) fn validate_expression(expr: &Expr, types: &ResultTypes) -> Result<(), CassieError> {
    if !contains_expression(expr) {
        return Ok(());
    }
    expr.try_visit_children(|child| validate_expression(child, types))?;
    if let Expr::Function(function) = expr {
        validate_function_type(function, types)?;
    }
    Ok(())
}

fn validate_function_type(function: &FunctionCall, types: &ResultTypes) -> Result<(), CassieError> {
    if is_conditional(&function.name) {
        let arguments = function
            .args
            .iter()
            .map(|arg| types.expression_type(arg).unwrap_or(DataType::Null))
            .collect::<Vec<_>>();
        result_type(&function.name, &arguments)?;
    }
    Ok(())
}

pub(super) fn validate_item(item: &SelectItem, types: &ResultTypes) -> Result<(), CassieError> {
    match item {
        SelectItem::Function { function, .. } => {
            for arg in &function.args {
                validate_expression(arg, types)?;
            }
            validate_function_type(function, types)?;
        }
        SelectItem::Expr { expr, .. } => validate_expression(expr, types)?,
        SelectItem::WindowFunction { function, .. } => {
            for expr in function
                .args
                .iter()
                .chain(&function.partition_by)
                .chain(function.order_by.iter().map(|order| &order.expr))
            {
                validate_expression(expr, types)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn coerce_expression(expr: &mut Expr, types: &ResultTypes) {
    if !contains_expression(expr) {
        return;
    }
    let mut rewritten = expr.map_children(|child| {
        let mut child = child.clone();
        coerce_expression(&mut child, types);
        child
    });
    if let Expr::Function(function) = &mut rewritten {
        coerce_function(function, types);
    }
    *expr = rewritten;
}

pub(super) fn coerce_function(function: &mut FunctionCall, scope: &ResultTypes) {
    if !is_conditional(&function.name) {
        return;
    }
    let types = function
        .args
        .iter()
        .map(|argument| scope.expression_type(argument).unwrap_or(DataType::Null))
        .collect::<Vec<_>>();
    if result_type(&function.name, &types).ok() != Some(DataType::Float) {
        return;
    }
    for (argument, data_type) in function.args.iter_mut().zip(types) {
        if matches!(
            data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
        ) {
            *argument = super::result_coercion::float(argument.clone());
        }
    }
}

pub(super) fn coerce_item(item: &mut SelectItem, types: &ResultTypes) {
    match item {
        SelectItem::Function { function, .. } => {
            function
                .args
                .iter_mut()
                .for_each(|arg| coerce_expression(arg, types));
            coerce_function(function, types);
        }
        SelectItem::Expr { expr, .. } => coerce_expression(expr, types),
        SelectItem::WindowFunction { function, .. } => {
            function
                .args
                .iter_mut()
                .chain(&mut function.partition_by)
                .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                .for_each(|expr| coerce_expression(expr, types));
        }
        _ => {}
    }
}

fn contains_expression(expr: &Expr) -> bool {
    matches!(expr, Expr::Function(function) if is_conditional(&function.name))
        || expr.any_child(contains_expression)
}
