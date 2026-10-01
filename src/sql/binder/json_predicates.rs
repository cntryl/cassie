use crate::catalog::CollectionSchema;
use crate::sql::ast::{BinaryOp, Expr, SelectItem, SelectStatement};
use crate::types::DataType;

pub(super) fn rewrite_select(select: &mut SelectStatement, fields: &crate::sql::FieldTypeMap) {
    for item in &mut select.projection {
        match item {
            SelectItem::Expr { expr, .. } => rewrite(expr, fields),
            SelectItem::Function { function, .. } => {
                for argument in &mut function.args {
                    rewrite(argument, fields);
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expression in function.args.iter_mut().chain(&mut function.partition_by) {
                    rewrite(expression, fields);
                }
                for order in &mut function.order_by {
                    rewrite(&mut order.expr, fields);
                }
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
    }
    for expression in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
    {
        rewrite(expression, fields);
    }
    for order in &mut select.order {
        rewrite(&mut order.expr, fields);
    }
}

pub(super) fn rewrite_filter(filter: Option<&mut Expr>, schema: &CollectionSchema) {
    if let Some(filter) = filter {
        let fields = schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect();
        rewrite(filter, &fields);
    }
}

pub(super) fn rewrite(expr: &mut Expr, fields: &crate::sql::FieldTypeMap) {
    *expr = rewritten(expr, fields);
}

fn rewritten(expr: &Expr, fields: &crate::sql::FieldTypeMap) -> Expr {
    let mut expr = expr.map_children(|child| rewritten(child, fields));
    match &mut expr {
        Expr::Binary {
            left,
            op: BinaryOp::Eq | BinaryOp::NotEq,
            right,
        } => {
            if matches!(right.as_ref(), Expr::StringLiteral(_)) {
                cast_column(left, fields);
            }
            if matches!(left.as_ref(), Expr::StringLiteral(_)) {
                cast_column(right, fields);
            }
        }
        Expr::InList { expr, values, .. }
            if values
                .iter()
                .any(|value| matches!(value, Expr::StringLiteral(_))) =>
        {
            cast_column(expr, fields);
        }
        _ => {}
    }
    expr
}

fn cast_column(expr: &mut Expr, fields: &crate::sql::FieldTypeMap) {
    if let Expr::Column(column) = expr {
        if matches!(
            crate::sql::field_type_for_column(fields, column),
            Some(DataType::Json)
        ) {
            *expr = Expr::Cast {
                expr: Box::new(expr.clone()),
                data_type: DataType::Json,
            };
        }
    }
}
