use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::batch::{chunk_rows, flatten_batches, row_tie_key, Batch, DEFAULT_BATCH_SIZE};
use crate::executor::filter;
use crate::executor::filter::SearchContext;
use crate::sql::ast::{Expr, NullsOrder, OrderExpr, SelectItem, SortDirection};
use crate::types::Value;

pub(crate) fn sort_rows<R>(
    mut rows: Vec<R>,
    order: &[OrderExpr],
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Vec<R>
where
    R: RowAccess,
{
    if order.is_empty() {
        return rows;
    }

    rows.sort_by(|left, right| {
        for OrderExpr {
            expr,
            direction,
            nulls,
        } in order
        {
            let left_value = sort_value(
                left,
                expr,
                projection,
                params,
                search_context,
                user_functions,
                session,
            );
            let right_value = sort_value(
                right,
                expr,
                projection,
                params,
                search_context,
                user_functions,
                session,
            );

            if let Some(cmp) = compare_nulls(&left_value, &right_value, *nulls) {
                return cmp;
            }

            let cmp = compare_scalar(&left_value, &right_value);
            if cmp != std::cmp::Ordering::Equal {
                return match direction {
                    SortDirection::Asc => cmp,
                    SortDirection::Desc => cmp.reverse(),
                };
            }
        }

        let left_key = row_tie_key(left);
        let right_key = row_tie_key(right);
        left_key.cmp(&right_key)
    });

    rows
}

pub(crate) fn sort_batches(
    batches: Vec<Batch>,
    order: &[OrderExpr],
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Vec<Batch> {
    if order.is_empty() {
        return batches;
    }

    let rows = flatten_batches(batches);
    let rows = sort_rows(
        rows,
        order,
        projection,
        params,
        search_context,
        user_functions,
        session,
    );
    chunk_rows(rows, DEFAULT_BATCH_SIZE)
}

fn sort_value<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> crate::executor::filter::ScalarValue {
    let base = filter::eval_scalar(
        row,
        expr,
        params,
        search_context,
        user_functions,
        None,
        session,
    )
    .unwrap_or(crate::executor::filter::ScalarValue::Null);
    if !matches!(base, crate::executor::filter::ScalarValue::Null) {
        return base;
    }

    alias_expr(expr, projection).map_or(base, |alias_expr| {
        filter::eval_scalar(
            row,
            &alias_expr,
            params,
            search_context,
            user_functions,
            None,
            session,
        )
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

fn compare_nulls(
    left: &crate::executor::filter::ScalarValue,
    right: &crate::executor::filter::ScalarValue,
    nulls: Option<NullsOrder>,
) -> Option<std::cmp::Ordering> {
    if let Some(nulls) = nulls {
        let left_null = matches!(left, crate::executor::filter::ScalarValue::Null);
        let right_null = matches!(right, crate::executor::filter::ScalarValue::Null);
        if left_null != right_null {
            return Some(match (left_null, nulls) {
                (true, NullsOrder::First) | (false, NullsOrder::Last) => std::cmp::Ordering::Less,
                (true, NullsOrder::Last) | (false, NullsOrder::First) => {
                    std::cmp::Ordering::Greater
                }
            });
        }
    }

    None
}
