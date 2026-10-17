//! Preserve declared function identities before builtin evaluation and folding.
use super::super::coalesce_results::ResultTypes;
use crate::sql::ast::{Expr, FunctionCall, SelectItem, SelectStatement};
use crate::sql::functions::{self, FunctionInputSignature};
pub(super) fn needs_qualification(select: &SelectStatement) -> bool {
    let contains = |expression: &Expr| {
        expression.any_descendant_or_self(&mut |expression| {
            matches!(expression, Expr::Function(function) if needs_function_qualification(function))
        })
    };
    select.projection.iter().any(|item| match item {
        SelectItem::Function { function, .. } => {
            needs_function_qualification(function) || function.expressions().any(contains)
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
    if needs_function_qualification(function)
        && types
            .declared_function(&function.name)
            .is_some_and(|metadata| {
                !functions::has_distinct_builtin_arity(
                    &function.name,
                    function.args.len(),
                    metadata.args.len(),
                )
            })
    {
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

fn needs_function_qualification(function: &FunctionCall) -> bool {
    functions::function(&function.name).is_some_and(|metadata| {
        functions::is_bytea_function(metadata.id)
            || metadata.id == functions::FunctionId::Lower
            || matches!(
                functions::input_signature(metadata.id),
                Some(FunctionInputSignature::Boolean)
            )
    })
}
