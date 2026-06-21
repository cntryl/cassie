use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, HashMap};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::sql::ast::{SelectItem, SortDirection};
use crate::types::Value;

use super::{compare_query_values, value_sort_key, QueryError};

pub(super) fn apply_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let windows = projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::WindowFunction { function, alias } => Some((function, alias)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if windows.is_empty() {
        return Ok(batches);
    }

    let mut rows = batch::flatten_batches(batches);
    for (function, alias) in windows {
        let function_name = function.name.to_ascii_lowercase();
        if !matches!(
            function_name.as_str(),
            "row_number" | "rank" | "dense_rank" | "lag" | "lead" | "first_value" | "last_value"
        ) {
            return Err(QueryError::General(format!(
                "unsupported window function '{}'",
                function.name
            )));
        }
        let output_name = alias
            .as_deref()
            .unwrap_or(function.name.as_str())
            .to_string();
        let mut partitions = BTreeMap::<String, Vec<usize>>::new();
        for (index, row) in rows.iter().enumerate() {
            let key = if function.partition_by.is_empty() {
                "__all__".to_string()
            } else {
                function
                    .partition_by
                    .iter()
                    .map(|expr| {
                        filter::evaluate_expr_value(
                            row,
                            expr,
                            params,
                            search_context,
                            user_functions,
                            session,
                            None,
                        )
                        .map(|value| value_sort_key(&value))
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("|")
            };
            partitions.entry(key).or_default().push(index);
        }

        let mut values = vec![Value::Null; rows.len()];
        for indices in partitions.values_mut() {
            indices.sort_by(|left, right| {
                compare_window_rows(
                    &rows[*left],
                    &rows[*right],
                    &function.order_by,
                    params,
                    search_context,
                    user_functions,
                    session,
                )
            });
            let mut dense_rank = 1i64;
            let mut previous_peer_key: Option<String> = None;
            for (position, index) in indices.iter().enumerate() {
                let peer_key = window_peer_key(
                    &rows[*index],
                    &function.order_by,
                    params,
                    search_context,
                    user_functions,
                    session,
                )?;
                if position > 0 && previous_peer_key.as_ref() != Some(&peer_key) {
                    dense_rank += 1;
                }
                previous_peer_key = Some(peer_key);

                values[*index] = match function_name.as_str() {
                    "row_number" => Value::Int64(i64::try_from(position + 1).unwrap_or(i64::MAX)),
                    "rank" => {
                        let peer_position = indices[..=position]
                            .iter()
                            .position(|candidate| {
                                window_peer_key(
                                    &rows[*candidate],
                                    &function.order_by,
                                    params,
                                    search_context,
                                    user_functions,
                                    session,
                                )
                                .ok()
                                    == previous_peer_key
                            })
                            .unwrap_or(position);
                        Value::Int64(i64::try_from(peer_position + 1).unwrap_or(i64::MAX))
                    }
                    "dense_rank" => Value::Int64(dense_rank),
                    "lag" => window_arg_value(
                        indices.get(position.wrapping_sub(1)).copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "lead" => window_arg_value(
                        indices.get(position + 1).copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "first_value" => window_arg_value(
                        indices.first().copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "last_value" => window_arg_value(
                        indices.last().copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    _ => Value::Null,
                };
            }
        }

        for (row, value) in rows.iter_mut().zip(values) {
            let mut entries = row.clone().into_entries();
            entries.push((output_name.clone(), value));
            *row = BatchRow::new(entries);
        }
    }

    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

fn window_arg_value(
    index: Option<usize>,
    rows: &[BatchRow],
    function: &crate::sql::ast::WindowFunctionCall,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let Some(index) = index else {
        return Ok(Value::Null);
    };
    let Some(expr) = function.args.first() else {
        return Ok(Value::Null);
    };
    filter::evaluate_expr_value(
        &rows[index],
        expr,
        params,
        search_context,
        user_functions,
        session,
        None,
    )
}

fn window_peer_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<String, QueryError> {
    if order_by.is_empty() {
        return Ok("__all__".to_string());
    }
    order_by
        .iter()
        .map(|order| {
            filter::evaluate_expr_value(
                row,
                &order.expr,
                params,
                search_context,
                user_functions,
                session,
                None,
            )
            .map(|value| value_sort_key(&value))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn compare_window_rows(
    left: &BatchRow,
    right: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> CmpOrdering {
    for order in order_by {
        let left_value = filter::evaluate_expr_value(
            left,
            &order.expr,
            params,
            search_context,
            user_functions,
            session,
            None,
        )
        .unwrap_or(Value::Null);
        let right_value = filter::evaluate_expr_value(
            right,
            &order.expr,
            params,
            search_context,
            user_functions,
            session,
            None,
        )
        .unwrap_or(Value::Null);
        let cmp = compare_query_values(&left_value, &right_value);
        if cmp != CmpOrdering::Equal {
            return match order.direction {
                SortDirection::Asc => cmp,
                SortDirection::Desc => cmp.reverse(),
            };
        }
    }
    batch::row_tie_key(left).cmp(&batch::row_tie_key(right))
}
