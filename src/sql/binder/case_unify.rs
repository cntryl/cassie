//! PostgreSQL-style CASE result coercion.
//!
//! When a CASE mixes integer and float results, PostgreSQL resolves the whole
//! expression to `float8`, so an integer branch still yields a float value.
//! The binder makes that explicit by casting integer-typed results to FLOAT,
//! keeping the evaluated values consistent with the declared column type.

use super::{
    infer_expr_type, DataType, Expr, FieldSchema, HashMap, Schema, SelectItem, SelectStatement,
};

pub(super) fn unify_select_case_results(
    select: &mut SelectStatement,
    field_types: &crate::sql::FieldTypeMap,
) {
    let schema = Schema {
        fields: field_types
            .iter()
            .map(|(name, data_type)| FieldSchema {
                name: name.clone(),
                data_type: data_type.clone(),
                nullable: true,
            })
            .collect(),
    };
    let unify = |expr: &mut Expr| *expr = unify_case_results(expr, &schema);
    for item in &mut select.projection {
        match item {
            SelectItem::Expr { expr, .. } => unify(expr),
            SelectItem::Function { function, .. } => function.args.iter_mut().for_each(unify),
            SelectItem::WindowFunction { function, .. } => {
                function.args.iter_mut().for_each(unify);
                function.partition_by.iter_mut().for_each(unify);
                function
                    .order_by
                    .iter_mut()
                    .for_each(|order| unify(&mut order.expr));
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
    }
    select.filter.iter_mut().for_each(unify);
    select.having.iter_mut().for_each(unify);
    select.group_by.iter_mut().for_each(unify);
    select
        .order
        .iter_mut()
        .for_each(|order| unify(&mut order.expr));
}

fn unify_case_results(expr: &Expr, schema: &Schema) -> Expr {
    let expr = expr.map_children(|child| unify_case_results(child, schema));
    let Expr::Case {
        operand,
        branches,
        else_expr,
    } = &expr
    else {
        return expr;
    };
    let result_type = |result: &Expr| infer_expr_type(result, schema, &HashMap::new(), &[]);
    let results = || {
        branches
            .iter()
            .map(|(_, then)| then)
            .chain(else_expr.as_deref())
    };
    let has_float = results().any(|result| result_type(result) == Some(DataType::Float));
    let has_integer =
        results().any(|result| result_type(result).is_some_and(|data_type| is_integer(&data_type)));
    if !(has_float && has_integer) {
        return expr;
    }
    let widen = |result: &Expr| {
        if result_type(result).is_some_and(|data_type| is_integer(&data_type)) {
            Expr::Cast {
                expr: Box::new(result.clone()),
                data_type: DataType::Float,
            }
        } else {
            result.clone()
        }
    };
    Expr::Case {
        operand: operand.clone(),
        branches: branches
            .iter()
            .map(|(when, then)| (when.clone(), widen(then)))
            .collect(),
        else_expr: else_expr.as_deref().map(|result| Box::new(widen(result))),
    }
}

fn is_integer(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt | DataType::Int | DataType::BigInt
    )
}
