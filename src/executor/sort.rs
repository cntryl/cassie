use crate::executor::filter;
use crate::executor::filter::SearchContext;
use crate::sql::ast::{Expr, OrderExpr, SelectItem, SortDirection};
use crate::types::Value;

pub(crate) fn sort_rows(
    mut rows: Vec<Vec<(String, Value)>>,
    order: &[OrderExpr],
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
) -> Result<Vec<Vec<(String, Value)>>, crate::executor::executor::QueryError> {
    if order.is_empty() {
        return Ok(rows);
    }

    rows.sort_by(|left, right| {
        for OrderExpr { expr, direction } in order {
            let left_value = sort_value(left.as_slice(), expr, projection, params, search_context);
            let right_value =
                sort_value(right.as_slice(), expr, projection, params, search_context);

            let cmp = compare_scalar(&left_value, &right_value);
            if cmp != std::cmp::Ordering::Equal {
                return match direction {
                    SortDirection::Asc => cmp,
                    SortDirection::Desc => cmp.reverse(),
                };
            }
        }

        let left_key = row_to_tie_key(left);
        let right_key = row_to_tie_key(right);
        left_key.cmp(&right_key)
    });

    Ok(rows)
}

fn sort_value(
    row: &[(String, Value)],
    expr: &Expr,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
) -> crate::executor::filter::ScalarValue {
    let base = filter::eval_scalar(row, expr, params, search_context)
        .unwrap_or(crate::executor::filter::ScalarValue::Null);
    if !matches!(base, crate::executor::filter::ScalarValue::Null) {
        return base;
    }

    alias_expr(expr, projection).map_or(base, |alias_expr| {
        filter::eval_scalar(row, &alias_expr, params, search_context)
            .unwrap_or(crate::executor::filter::ScalarValue::Null)
    })
}

fn alias_expr(expr: &Expr, projection: &[SelectItem]) -> Option<Expr> {
    match expr {
        Expr::Column(alias) => projection.iter().find_map(|item| {
            let alias_lower = alias.to_ascii_lowercase();
            match item {
                SelectItem::Column {
                    name,
                    alias: Some(project_alias),
                    ..
                } if project_alias.to_ascii_lowercase() == alias_lower => {
                    Some(Expr::Column(name.clone()))
                }
                SelectItem::Function {
                    function,
                    alias: Some(project_alias),
                    ..
                } if project_alias.to_ascii_lowercase() == alias_lower => {
                    Some(Expr::Function(function.clone()))
                }
                _ => None,
            }
        }),
        _ => None,
    }
}

fn compare_scalar(
    left: &crate::executor::filter::ScalarValue,
    right: &crate::executor::filter::ScalarValue,
) -> std::cmp::Ordering {
    if let (Some(left), Some(right)) = (left.to_f64(), right.to_f64()) {
        return left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal);
    }

    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left.cmp(right);
    }

    std::cmp::Ordering::Equal
}

fn row_to_tie_key(row: &[(String, Value)]) -> String {
    row.iter()
        .map(|(_, value)| value_to_key(value))
        .collect::<Vec<_>>()
        .join("|")
}

fn value_to_key(value: &Value) -> String {
    match value {
        Value::Null => String::from("<null>"),
        Value::Bool(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Float64(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::Vector(v) => v
            .values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Json(v) => v.to_string(),
    }
}
