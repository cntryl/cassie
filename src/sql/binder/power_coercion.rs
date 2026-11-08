//! Resolve unknown power literals before strict evaluation drops NULL inputs.
use super::coalesce_results::ResultTypes;
use super::{DataType, Expr, FunctionCall};
use crate::sql::ast::CastIdentity;

pub(super) fn function(function: &mut FunctionCall, types: &ResultTypes) -> [bool; 2] {
    let Some(metadata) = crate::sql::functions::function(&function.name) else {
        return [false; 2];
    };
    if metadata.id != crate::sql::functions::FunctionId::Power || function.args.len() != 2 {
        return [false; 2];
    }
    if types
        .declared_function(&function.name)
        .is_some_and(|routine| {
            !crate::sql::functions::has_distinct_builtin_arity(
                &function.name,
                function.args.len(),
                routine.args.len(),
            )
        })
    {
        return [false; 2];
    }
    let [left, right] = function.args.as_mut_slice() else {
        return [false; 2];
    };
    pair(left, right, types)
}

pub(super) fn pair(left: &mut Expr, right: &mut Expr, types: &ResultTypes) -> [bool; 2] {
    let left_type = types.expression_type(left).unwrap_or(DataType::Null);
    let right_type = types.expression_type(right).unwrap_or(DataType::Null);
    // Wait for the concrete parameter pass before resolving an unknown literal
    // beside a parameter whose type has not yet been determined.
    if (matches!(left, Expr::StringLiteral(_))
        && matches!(right_type, DataType::Null)
        && unresolved_parameter(right, types))
        || (matches!(right, Expr::StringLiteral(_))
            && matches!(left_type, DataType::Null)
            && unresolved_parameter(left, types))
    {
        return [false; 2];
    }
    let selected = crate::types::extended::numeric_types::arithmetic(&left_type, &right_type, true)
        .unwrap_or(DataType::Float);
    let mut changed = [false; 2];
    for (index, operand) in [left, right].into_iter().enumerate() {
        if matches!(operand, Expr::StringLiteral(_)) {
            changed[index] = true;
            let original = std::mem::replace(operand, Expr::Null);
            let identity = if matches!(
                selected,
                DataType::Extended(crate::types::extended::ExtendedType::Numeric(_))
            ) {
                CastIdentity::Numeric
            } else {
                CastIdentity::Scalar
            };
            *operand = Expr::Cast {
                expr: Box::new(original),
                data_type: selected.clone(),
                identity,
                source_type: None,
            };
        }
    }
    changed
}

fn unresolved_parameter(expr: &Expr, types: &ResultTypes) -> bool {
    expr.any_descendant_or_self(&mut |node| match node {
        Expr::Param(index) => types
            .parameter_types()
            .get(*index)
            .copied()
            .is_none_or(|oid| matches!(oid, 0 | 705)),
        _ => false,
    })
}
