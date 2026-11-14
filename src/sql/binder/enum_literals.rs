//! Unknown literals that meet an enum in a conditional expression (#827).
//!
//! PostgreSQL resolves an unknown-typed string literal to the type of the
//! other inputs, so `CASE e WHEN 'a'`, `coalesce(e, 'a')` and
//! `greatest(e, 'c')` read the literal as a label of `e`'s enum. The binder
//! makes that explicit with a cast before type checking, and a label that is
//! not in the enum is `22P02`, as `enum_in` fails when the literal resolves.

use super::{DataType, Expr, SelectItem, SelectStatement};
use crate::app::CassieError;
use crate::sql::ast::CastIdentity;
use crate::sql::FieldTypeMap;
use crate::types::EnumRef;

const CONDITIONAL_FUNCTIONS: &[&str] = &["coalesce", "greatest", "least", "nullif"];

fn enum_of(expr: &Expr, field_types: &FieldTypeMap) -> Option<EnumRef> {
    match super::inference::known_expr_type(expr, field_types)?.flattened() {
        DataType::Enum(descriptor) => Some(descriptor.clone()),
        _ => None,
    }
}

/// Casts `expr` to `target` when it is an unknown string literal.
fn cast_literal(expr: &mut Expr, target: &EnumRef) -> Result<(), CassieError> {
    let Expr::StringLiteral(label) = expr else {
        return Ok(());
    };
    crate::types::EnumValue::parse_label(target, label).map_err(|error| {
        CassieError::from(crate::executor::filter::enum_cast::input_diagnostic(&error))
    })?;
    let literal = std::mem::replace(expr, Expr::Null);
    *expr = Expr::Cast {
        expr: Box::new(literal),
        data_type: DataType::Enum(target.clone()),
        source_type: None,
        identity: CastIdentity::Scalar,
    };
    Ok(())
}

fn is_conditional(name: &str) -> bool {
    let local = name.rsplit('.').next().unwrap_or(name);
    CONDITIONAL_FUNCTIONS
        .iter()
        .any(|candidate| local.eq_ignore_ascii_case(candidate))
}

fn rewrite_arguments(args: &mut [Expr], field_types: &FieldTypeMap) -> Result<(), CassieError> {
    let Some(target) = args.iter().find_map(|arg| enum_of(arg, field_types)) else {
        return Ok(());
    };
    args.iter_mut()
        .try_for_each(|argument| cast_literal(argument, &target))
}

fn rewrite_expression(expr: &mut Expr, field_types: &FieldTypeMap) -> Result<(), CassieError> {
    expr.try_visit_children_mut(|child| rewrite_expression(child, field_types))?;
    match expr {
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            if let Some(target) = operand
                .as_deref()
                .and_then(|operand| enum_of(operand, field_types))
            {
                for (when, _) in branches.iter_mut() {
                    cast_literal(when, &target)?;
                }
            }
            let result = branches
                .iter()
                .map(|(_, then)| then)
                .chain(else_expr.as_deref())
                .find_map(|result| enum_of(result, field_types));
            if let Some(target) = result {
                for (_, then) in branches.iter_mut() {
                    cast_literal(then, &target)?;
                }
                if let Some(result) = else_expr {
                    cast_literal(result, &target)?;
                }
            }
        }
        Expr::Function(function) if is_conditional(&function.name) => {
            rewrite_arguments(&mut function.args, field_types)?;
        }
        _ => {}
    }
    Ok(())
}

/// Rewrites the expressions of one bound `SELECT` level.
pub(super) fn rewrite_select(
    select: &mut SelectStatement,
    field_types: &FieldTypeMap,
) -> Result<(), CassieError> {
    for item in &mut select.projection {
        match item {
            SelectItem::Expr { expr, .. } => rewrite_expression(expr, field_types)?,
            SelectItem::Function { function, .. } => {
                for expression in function.expressions_mut() {
                    rewrite_expression(expression, field_types)?;
                }
                if is_conditional(&function.name) {
                    rewrite_arguments(&mut function.args, field_types)?;
                }
            }
            SelectItem::Column { .. }
            | SelectItem::Wildcard
            | SelectItem::WindowFunction { .. } => {}
        }
    }
    for expression in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
    {
        rewrite_expression(expression, field_types)?;
    }
    for order in &mut select.order {
        rewrite_expression(&mut order.expr, field_types)?;
    }
    Ok(())
}
