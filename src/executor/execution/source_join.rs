use super::*;

#[derive(Debug, Clone)]
struct EquiJoinKeys {
    left: String,
    right: String,
}

#[derive(Debug)]
struct KeyedRow {
    key: String,
    row: BatchRow,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_join_source<'a>(
    env: &'a SourceExecutionEnv<'a>,
    left: &'a QuerySource,
    right: &'a QuerySource,
    kind: &'a JoinKind,
    on: &'a Expr,
    cte_context: &'a mut CteContext,
    outer_row: Option<&'a BatchRow>,
) -> SourceExecution<'a> {
    let (left_batches, _left_text) = execute_query_source(env, left, cte_context, true, outer_row)?;
    if source_contains_lateral(right) {
        return execute_lateral_join(env, right, kind, on, cte_context, outer_row, left_batches);
    }

    let (right_batches, _right_text) =
        execute_query_source(env, right, cte_context, true, outer_row)?;
    let left_rows = batch::flatten_batches(left_batches);
    let right_rows = batch::flatten_batches(right_batches);
    let left_columns = row_columns(&left_rows);
    let right_columns = row_columns(&right_rows);

    let joined = merge_join_keys(on, &left_columns, &right_columns)
        .filter(|_| !matches!(kind, JoinKind::Cross))
        .map(|keys| {
            execute_merge_join(
                env,
                kind,
                on,
                keys,
                left_rows.clone(),
                right_rows.clone(),
                &left_columns,
                &right_columns,
            )
        })
        .transpose()?
        .map(Ok)
        .unwrap_or_else(|| {
            execute_nested_loop_join(
                env,
                kind,
                on,
                left_rows,
                right_rows,
                &left_columns,
                &right_columns,
            )
        })?;

    finish_join(joined)
}

#[allow(clippy::too_many_arguments)]
fn execute_lateral_join<'a>(
    env: &'a SourceExecutionEnv<'a>,
    right: &'a QuerySource,
    kind: &'a JoinKind,
    on: &'a Expr,
    cte_context: &'a mut CteContext,
    _outer_row: Option<&'a BatchRow>,
    left_batches: Vec<Batch>,
) -> SourceExecution<'a> {
    let left_rows = batch::flatten_batches(left_batches);
    let mut joined = Vec::new();
    let mut matched_rows = 0usize;

    for left_row in &left_rows {
        let (right_batches, _right_text) =
            execute_query_source(env, right, cte_context, true, Some(left_row))?;
        let right_rows = batch::flatten_batches(right_batches);
        let right_columns = row_columns(&right_rows);
        let mut matched = false;
        for right_row in &right_rows {
            let combined = combine_rows(left_row, right_row);
            let passes = matches!(kind, JoinKind::Cross)
                || filter::eval_scalar(
                    &combined,
                    on,
                    env.params,
                    None,
                    env.user_functions,
                    None,
                    env.session,
                )?
                .as_bool();
            if passes {
                matched = true;
                matched_rows += 1;
                joined.push(combined);
            }
        }

        if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
            joined.push(combine_row_with_nulls(left_row, &right_columns));
        }
    }

    env.cassie.runtime.record_join_execution(
        "nested_loop",
        left_rows.len(),
        0,
        matched_rows,
        joined.len(),
        None,
    );
    finish_join(joined)
}

#[allow(clippy::too_many_arguments)]
fn execute_nested_loop_join(
    env: &SourceExecutionEnv<'_>,
    kind: &JoinKind,
    on: &Expr,
    left_rows: Vec<BatchRow>,
    right_rows: Vec<BatchRow>,
    left_columns: &[String],
    right_columns: &[String],
) -> Result<Vec<BatchRow>, QueryError> {
    let mut joined = Vec::new();
    let mut right_matched = vec![false; right_rows.len()];
    let mut matched_rows = 0usize;

    for left_row in &left_rows {
        let mut matched = false;
        for (right_index, right_row) in right_rows.iter().enumerate() {
            let combined = combine_rows(left_row, right_row);
            let passes = matches!(kind, JoinKind::Cross)
                || filter::eval_scalar(
                    &combined,
                    on,
                    env.params,
                    None,
                    env.user_functions,
                    None,
                    env.session,
                )?
                .as_bool();
            if passes {
                matched = true;
                matched_rows += 1;
                right_matched[right_index] = true;
                joined.push(combined);
            }
        }

        if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
            joined.push(combine_row_with_nulls(left_row, right_columns));
        }
    }

    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        for (right_index, right_row) in right_rows.iter().enumerate() {
            if !right_matched[right_index] {
                joined.push(combine_nulls_with_row(left_columns, right_row));
            }
        }
    }

    env.cassie.runtime.record_join_execution(
        if matches!(kind, JoinKind::Cross) {
            "cross"
        } else {
            "nested_loop"
        },
        left_rows.len(),
        right_rows.len(),
        matched_rows,
        joined.len(),
        None,
    );
    Ok(joined)
}

#[allow(clippy::too_many_arguments)]
fn execute_merge_join(
    env: &SourceExecutionEnv<'_>,
    kind: &JoinKind,
    on: &Expr,
    keys: EquiJoinKeys,
    left_rows: Vec<BatchRow>,
    right_rows: Vec<BatchRow>,
    left_columns: &[String],
    right_columns: &[String],
) -> Result<Vec<BatchRow>, QueryError> {
    let left_len = left_rows.len();
    let right_len = right_rows.len();
    let mut left_keyed = keyed_rows(left_rows, &keys.left);
    let mut right_keyed = keyed_rows(right_rows, &keys.right);
    left_keyed.sort_by(|left, right| left.key.cmp(&right.key));
    right_keyed.sort_by(|left, right| left.key.cmp(&right.key));

    let mut joined = Vec::new();
    let mut matched_rows = 0usize;
    let mut left_index = 0usize;
    let mut right_index = 0usize;

    while left_index < left_keyed.len() && right_index < right_keyed.len() {
        match left_keyed[left_index]
            .key
            .cmp(&right_keyed[right_index].key)
        {
            std::cmp::Ordering::Less => {
                let group_end = keyed_group_end(&left_keyed, left_index);
                if matches!(kind, JoinKind::Left | JoinKind::Full) {
                    for left in &left_keyed[left_index..group_end] {
                        joined.push(combine_row_with_nulls(&left.row, right_columns));
                    }
                }
                left_index = group_end;
            }
            std::cmp::Ordering::Greater => {
                let group_end = keyed_group_end(&right_keyed, right_index);
                if matches!(kind, JoinKind::Right | JoinKind::Full) {
                    for right in &right_keyed[right_index..group_end] {
                        joined.push(combine_nulls_with_row(left_columns, &right.row));
                    }
                }
                right_index = group_end;
            }
            std::cmp::Ordering::Equal => {
                let left_end = keyed_group_end(&left_keyed, left_index);
                let right_end = keyed_group_end(&right_keyed, right_index);
                let mut left_matched = vec![false; left_end - left_index];
                let mut right_matched = vec![false; right_end - right_index];

                for (left_offset, left) in left_keyed[left_index..left_end].iter().enumerate() {
                    for (right_offset, right) in
                        right_keyed[right_index..right_end].iter().enumerate()
                    {
                        let combined = combine_rows(&left.row, &right.row);
                        if filter::eval_scalar(
                            &combined,
                            on,
                            env.params,
                            None,
                            env.user_functions,
                            None,
                            env.session,
                        )?
                        .as_bool()
                        {
                            left_matched[left_offset] = true;
                            right_matched[right_offset] = true;
                            matched_rows += 1;
                            joined.push(combined);
                        }
                    }
                }

                if matches!(kind, JoinKind::Left | JoinKind::Full) {
                    for (offset, matched) in left_matched.iter().enumerate() {
                        if !matched {
                            joined.push(combine_row_with_nulls(
                                &left_keyed[left_index + offset].row,
                                right_columns,
                            ));
                        }
                    }
                }
                if matches!(kind, JoinKind::Right | JoinKind::Full) {
                    for (offset, matched) in right_matched.iter().enumerate() {
                        if !matched {
                            joined.push(combine_nulls_with_row(
                                left_columns,
                                &right_keyed[right_index + offset].row,
                            ));
                        }
                    }
                }

                left_index = left_end;
                right_index = right_end;
            }
        }
    }

    if matches!(kind, JoinKind::Left | JoinKind::Full) {
        for left in &left_keyed[left_index..] {
            joined.push(combine_row_with_nulls(&left.row, right_columns));
        }
    }
    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        for right in &right_keyed[right_index..] {
            joined.push(combine_nulls_with_row(left_columns, &right.row));
        }
    }

    env.cassie.runtime.record_join_execution(
        "merge",
        left_len,
        right_len,
        matched_rows,
        joined.len(),
        None,
    );
    Ok(joined)
}

fn keyed_rows(rows: Vec<BatchRow>, key_column: &str) -> Vec<KeyedRow> {
    rows.into_iter()
        .enumerate()
        .map(|(_index, row)| {
            let key = row
                .get(key_column)
                .map(value_sort_key)
                .unwrap_or_else(|| value_sort_key(&Value::Null));
            KeyedRow { key, row }
        })
        .collect()
}

fn keyed_group_end(rows: &[KeyedRow], start: usize) -> usize {
    let key = rows[start].key.as_str();
    let mut end = start + 1;
    while end < rows.len() && rows[end].key == key {
        end += 1;
    }
    end
}

fn merge_join_keys(
    on: &Expr,
    left_columns: &[String],
    right_columns: &[String],
) -> Option<EquiJoinKeys> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = on
    else {
        return None;
    };
    let (Expr::Column(left_name), Expr::Column(right_name)) = (left.as_ref(), right.as_ref())
    else {
        return None;
    };

    if column_belongs_to(left_name, left_columns, right_columns)
        && column_belongs_to(right_name, right_columns, left_columns)
    {
        return Some(EquiJoinKeys {
            left: left_name.clone(),
            right: right_name.clone(),
        });
    }

    if column_belongs_to(right_name, left_columns, right_columns)
        && column_belongs_to(left_name, right_columns, left_columns)
    {
        return Some(EquiJoinKeys {
            left: right_name.clone(),
            right: left_name.clone(),
        });
    }

    None
}

fn column_belongs_to(name: &str, own_columns: &[String], other_columns: &[String]) -> bool {
    own_columns.iter().any(|column| column == name)
        && (!other_columns.iter().any(|column| column == name) || name.contains('.'))
}

fn finish_join<'a>(joined: Vec<BatchRow>) -> SourceExecution<'a> {
    let text_fields = deduce_text_fields(
        &joined
            .iter()
            .map(|row| row.entries().to_vec())
            .collect::<Vec<_>>(),
    );
    let batches = batch::chunk_rows(joined, batch::DEFAULT_BATCH_SIZE);
    Ok((batches, text_fields))
}
