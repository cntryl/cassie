//! Validate the fixed Boolean aggregate signature with the shared result resolver.

use super::super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::{Expr, FunctionCall, SelectItem, SelectStatement};
use crate::sql::functions::{self, FunctionInputSignature};
use crate::types::DataType;

pub(super) fn needs_qualification(select: &SelectStatement) -> bool {
    let contains = |expression: &Expr| {
        expression.any_descendant_or_self(&mut |expression| {
            matches!(expression, Expr::Function(function) if is_boolean_aggregate(function))
        })
    };
    select.projection.iter().any(|item| match item {
        SelectItem::Function { function, .. } => {
            is_boolean_aggregate(function) || function.expressions().any(contains)
        }
        SelectItem::Expr { expr, .. } => contains(expr),
        SelectItem::WindowFunction { function, .. } => function.args.iter().any(contains),
        SelectItem::Column { .. } | SelectItem::Wildcard => false,
    }) || select
        .filter
        .iter()
        .chain(&select.having)
        .chain(&select.group_by)
        .chain(&select.distinct_on)
        .chain(select.order.iter().map(|order| &order.expr))
        .chain(&select.limit)
        .chain(&select.offset)
        .any(contains)
}

pub(super) fn qualify_declared(select: &mut SelectStatement, types: &ResultTypes) {
    for item in &mut select.projection {
        match item {
            SelectItem::Function { function, alias } => {
                qualify_function(function, types, Some(alias));
            }
            SelectItem::Expr { expr, .. } => qualify_expression(expr, types),
            SelectItem::WindowFunction { function, .. } => {
                for expression in function
                    .args
                    .iter_mut()
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    qualify_expression(expression, types);
                }
            }
            SelectItem::Column { .. } | SelectItem::Wildcard => {}
        }
    }
    for expression in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
        .chain(&mut select.limit)
        .chain(&mut select.offset)
    {
        qualify_expression(expression, types);
    }
}

fn qualify_function(
    function: &mut FunctionCall,
    types: &ResultTypes,
    alias: Option<&mut Option<String>>,
) {
    if is_boolean_aggregate(function) {
        if let Some(identity) = types.declared_function_identity(&function.name) {
            let previous = std::mem::replace(&mut function.name, identity.to_owned());
            if let Some(alias @ None) = alias {
                *alias = Some(previous);
            }
        }
    }
    for argument in function.expressions_mut() {
        qualify_expression(argument, types);
    }
}

fn qualify_expression(expression: &mut Expr, types: &ResultTypes) {
    match expression {
        Expr::Function(function) => qualify_function(function, types, None),
        Expr::RowCompare(comparison) => comparison
            .left
            .iter_mut()
            .chain(&mut comparison.right)
            .for_each(|field| qualify_expression(field, types)),
        Expr::Binary { left, right, .. } => {
            qualify_expression(left, types);
            qualify_expression(right, types);
        }
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            if let Some(operand) = operand {
                qualify_expression(operand, types);
            }
            for (condition, value) in branches {
                qualify_expression(condition, types);
                qualify_expression(value, types);
            }
            if let Some(value) = else_expr {
                qualify_expression(value, types);
            }
        }
        Expr::Cast { expr, .. }
        | Expr::UnaryNumeric { expr, .. }
        | Expr::IntegerNegate { expr, .. }
        | Expr::Not { expr }
        | Expr::IsNull { expr, .. }
        | Expr::TruthTest { expr, .. } => {
            qualify_expression(expr, types);
        }
        Expr::InList { expr, values, .. } => {
            qualify_expression(expr, types);
            for value in values {
                qualify_expression(value, types);
            }
        }
        Expr::Between {
            expr, low, high, ..
        }
        | Expr::Like {
            expr,
            pattern: low,
            escape: high,
        } => {
            qualify_expression(expr, types);
            qualify_expression(low, types);
            qualify_expression(high, types);
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Exists(_) => {}
    }
}

fn is_boolean_aggregate(function: &FunctionCall) -> bool {
    functions::function(&function.name).is_some_and(|metadata| {
        metadata.is_aggregate
            && matches!(
                functions::input_signature(metadata.id),
                Some(FunctionInputSignature::Boolean)
            )
    })
}

pub(super) fn validate(
    function: &mut FunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if !is_boolean_aggregate(function) {
        return Ok(());
    }
    if function.args.len() != 1 {
        return Err(undefined_function(function, types));
    }
    let argument = &mut function.args[0];
    if matches!(argument, Expr::Column(name) if name == "*") {
        return Err(CassieError::Execution(format!(
            "function {}() does not exist",
            function.name.to_ascii_lowercase()
        )));
    }
    if let Expr::StringLiteral(text) = argument {
        let value = crate::types::boolean::parse_text(text).ok_or_else(|| {
            CassieError::Execution(format!("invalid input syntax for type boolean: {text:?}"))
        })?;
        *argument = Expr::BoolLiteral(value);
    }
    match types.expression_type(argument) {
        Some(DataType::Boolean | DataType::Null) | None => Ok(()),
        Some(_) => Err(undefined_function(function, types)),
    }
}

pub(super) fn undefined_function(function: &FunctionCall, types: &ResultTypes) -> CassieError {
    let arguments = function
        .args
        .iter()
        .map(|argument| {
            types
                .expression_type(argument)
                .map_or_else(|| "unknown".to_owned(), |data_type| type_name(&data_type))
        })
        .collect::<Vec<_>>()
        .join(", ");
    CassieError::Execution(format!(
        "function {}({arguments}) does not exist",
        function.name.to_ascii_lowercase()
    ))
}

pub(super) fn type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "unknown".to_owned(),
        DataType::Int => "integer".to_owned(),
        DataType::Float => "double precision".to_owned(),
        DataType::Char { .. } => "character".to_owned(),
        DataType::Varchar { .. } => "character varying".to_owned(),
        DataType::Array(inner) => format!("{}[]", type_name(inner)),
        other => other.type_name(),
    }
}
