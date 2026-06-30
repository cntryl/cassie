use super::{
    batch, catalog, check_timeout, combine_nulls_with_row, combine_row_with_nulls, combine_rows,
    deduce_text_fields, execute_query_source, filter, qualify_row, row_columns, scan,
    source_contains_lateral, value_sort_key, Batch, BatchRow, BinaryOp, CteContext, Expr, JoinKind,
    QueryError, QuerySource, SourceExecution, SourceExecutionEnv, Value,
};

const VECTOR_TO_MERGE_SWITCH_PAIR: &str = "vectorized_join_to_merge_join";

#[path = "source_join/bounded.rs"]
mod bounded;

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
    kind: JoinKind,
    on: &'a Expr,
    cte_context: &'a mut CteContext,
    outer_row: Option<&'a BatchRow>,
    row_budget: Option<usize>,
) -> SourceExecution<'a> {
    if !source_contains_lateral(right) {
        if let Some(joined) = bounded::try_execute_indexed_bounded_inner_join(
            env,
            left,
            right,
            kind,
            on,
            cte_context,
            outer_row,
            row_budget,
        )? {
            return Ok(finish_join(joined));
        }
        if let Some(joined) = bounded::try_execute_streaming_bounded_inner_join(
            env,
            left,
            right,
            kind,
            on,
            cte_context,
            outer_row,
            row_budget,
        )? {
            return Ok(finish_join(joined));
        }
    }

    let left_row_budget = if matches!(kind, JoinKind::Left) {
        row_budget
    } else {
        None
    };
    let (left_batches, _left_text) =
        execute_query_source(env, left, cte_context, true, outer_row, left_row_budget)?;
    if source_contains_lateral(right) {
        return execute_lateral_join(env, right, kind, on, cte_context, outer_row, left_batches);
    }

    let (right_batches, _right_text) =
        execute_query_source(env, right, cte_context, true, outer_row, None)?;
    let left_rows = batch::flatten_batches(left_batches);
    let right_rows = batch::flatten_batches(right_batches);
    let left_columns = row_columns(&left_rows);
    let right_columns = row_columns(&right_rows);

    let joined = match merge_join_keys(on, &left_columns, &right_columns)
        .filter(|_| !matches!(kind, JoinKind::Cross))
    {
        Some(keys) => execute_vectorized_join(
            env,
            kind,
            on,
            &keys,
            &left_rows,
            &right_rows,
            &right_columns,
            row_budget,
        )?
        .map_or_else(
            || {
                execute_merge_join(
                    env,
                    kind,
                    on,
                    &keys,
                    &left_rows,
                    &right_rows,
                    &left_columns,
                    &right_columns,
                )
            },
            Ok,
        )?,
        None => execute_nested_loop_join(
            env,
            kind,
            on,
            &left_rows,
            &right_rows,
            &left_columns,
            &right_columns,
        )?,
    };

    Ok(finish_join(joined))
}

#[allow(clippy::too_many_arguments)]
fn execute_lateral_join<'a>(
    env: &'a SourceExecutionEnv<'a>,
    right: &'a QuerySource,
    kind: JoinKind,
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
            execute_query_source(env, right, cte_context, true, Some(left_row), None)?;
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
    Ok(finish_join(joined))
}

#[allow(clippy::too_many_arguments)]
fn execute_nested_loop_join(
    env: &SourceExecutionEnv<'_>,
    kind: JoinKind,
    on: &Expr,
    left_rows: &[BatchRow],
    right_rows: &[BatchRow],
    left_columns: &[String],
    right_columns: &[String],
) -> Result<Vec<BatchRow>, QueryError> {
    let mut joined = Vec::new();
    let mut right_matched = vec![false; right_rows.len()];
    let mut matched_rows = 0usize;

    for left_row in left_rows {
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

fn collection_join_columns(env: &SourceExecutionEnv<'_>, collection: &str) -> Option<Vec<String>> {
    let mut columns = vec!["id".to_string()];
    columns.extend(collection_scan_fields(env, collection)?);
    Some(qualify_column_names(columns, collection))
}

fn collection_scan_fields(env: &SourceExecutionEnv<'_>, collection: &str) -> Option<Vec<String>> {
    Some(
        env.cassie
            .catalog
            .get_schema(collection)?
            .fields
            .into_iter()
            .map(|field| field.name)
            .collect(),
    )
}

fn qualify_column_names(columns: Vec<String>, qualifier: &str) -> Vec<String> {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut out = Vec::with_capacity(columns.len() * 2);
    for column in columns {
        out.push(column.clone());
        out.push(format!("{qualifier}.{column}"));
    }
    out
}

fn join_field_for_collection(key: &str, collection: &str) -> Option<String> {
    let key_lower = key.to_ascii_lowercase();
    let prefix = format!("{}.", collection.to_ascii_lowercase());
    if key_lower.starts_with(&prefix) {
        return Some(key[prefix.len()..].to_string());
    }
    (!key.contains('.')).then(|| key.to_string())
}

#[allow(clippy::too_many_arguments)]
fn execute_vectorized_join(
    env: &SourceExecutionEnv<'_>,
    kind: JoinKind,
    _on: &Expr,
    keys: &EquiJoinKeys,
    left_rows: &[BatchRow],
    right_rows: &[BatchRow],
    right_columns: &[String],
    row_budget: Option<usize>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(batch_size) = vectorized_join_batch_size(env, kind, left_rows, right_rows)? else {
        return Ok(None);
    };
    let output_budget = row_budget.unwrap_or(usize::MAX);
    if output_budget == 0 {
        env.cassie
            .runtime
            .record_vectorized_join_execution(0, 0, 0, 0, batch_size, 0);
        return Ok(Some(Vec::new()));
    }

    let mut build = std::collections::HashMap::<String, Vec<&BatchRow>>::new();
    for right in right_rows {
        let key = row_join_key(right, &keys.right);
        build.entry(key).or_default().push(right);
    }

    let mut probe_rows = 0usize;
    let build_rows = build.values().map(Vec::len).sum::<usize>();
    let mut batches = 0usize;
    let mut matched_rows = 0usize;
    let mut joined = Vec::new();

    'probe: for left_batch in left_rows.chunks(batch_size) {
        check_timeout(env.controls)?;
        batches += 1;
        for left in left_batch {
            probe_rows += 1;
            let key = row_join_key(left, &keys.left);
            let mut matched = false;
            if let Some(right_group) = build.get(&key) {
                for right in right_group {
                    matched = true;
                    matched_rows += 1;
                    joined.push(combine_rows(left, right));
                    if joined.len() >= output_budget {
                        break 'probe;
                    }
                }
            }

            if !matched && matches!(kind, JoinKind::Left) {
                joined.push(combine_row_with_nulls(left, right_columns));
                if joined.len() >= output_budget {
                    break 'probe;
                }
            }
        }
    }

    env.cassie.runtime.record_vectorized_join_execution(
        probe_rows,
        build_rows,
        matched_rows,
        joined.len(),
        batch_size,
        batches,
    );
    Ok(Some(joined))
}

#[allow(clippy::too_many_arguments)]
fn execute_merge_join(
    env: &SourceExecutionEnv<'_>,
    kind: JoinKind,
    on: &Expr,
    keys: &EquiJoinKeys,
    left_rows: &[BatchRow],
    right_rows: &[BatchRow],
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
                let result = merge_equal_key_groups(
                    env,
                    kind,
                    on,
                    &left_keyed[left_index..left_end],
                    &right_keyed[right_index..right_end],
                    left_columns,
                    right_columns,
                )?;
                matched_rows += result.matched_rows;
                joined.extend(result.joined);
                left_index = left_end;
                right_index = right_end;
            }
        }
    }

    append_left_unmatched(&mut joined, kind, &left_keyed[left_index..], right_columns);
    append_right_unmatched(&mut joined, kind, left_columns, &right_keyed[right_index..]);

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

fn keyed_rows(rows: &[BatchRow], key_column: &str) -> Vec<KeyedRow> {
    rows.iter()
        .cloned()
        .map(|row| {
            let key = row
                .get(key_column)
                .map_or_else(|| value_sort_key(&Value::Null), value_sort_key);
            KeyedRow { key, row }
        })
        .collect()
}

fn row_join_key(row: &BatchRow, key_column: &str) -> String {
    row.get(key_column)
        .map_or_else(|| value_sort_key(&Value::Null), value_sort_key)
}

fn estimate_vectorized_join_bytes(left_rows: usize, right_rows: usize) -> usize {
    left_rows
        .saturating_add(right_rows)
        .saturating_mul(std::mem::size_of::<BatchRow>().max(512))
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

fn finish_join(joined: Vec<BatchRow>) -> (Vec<Batch>, Vec<String>) {
    let text_fields = deduce_text_fields(
        &joined
            .iter()
            .map(|row| row.entries().to_vec())
            .collect::<Vec<_>>(),
    );
    let batches = batch::chunk_rows(joined, batch::DEFAULT_BATCH_SIZE);
    (batches, text_fields)
}

fn vectorized_join_batch_size(
    env: &SourceExecutionEnv<'_>,
    kind: JoinKind,
    left_rows: &[BatchRow],
    right_rows: &[BatchRow],
) -> Result<Option<usize>, QueryError> {
    let limits = env.cassie.runtime.limits();
    let batch_size = limits.vectorized_join_batch_size.max(1);
    if !limits.vectorized_joins_enabled {
        return Ok(None);
    }
    if !matches!(kind, JoinKind::Inner | JoinKind::Left) {
        if limits.operator_switching_enabled.is_enabled() {
            env.cassie.runtime.record_runtime_operator_switch_skip(
                VECTOR_TO_MERGE_SWITCH_PAIR,
                "unsupported_join_type",
                "rows_emitted=0",
            );
        }
        env.cassie.runtime.record_vectorized_join_fallback(
            "unsupported_join_type",
            batch_size,
            false,
        );
        return Ok(None);
    }

    let observed_rows = left_rows.len().saturating_add(right_rows.len());
    if limits.operator_switching_enabled.is_enabled()
        && observed_rows > limits.operator_switch_join_row_threshold
    {
        check_timeout(env.controls)?;
        let state = format!(
            "replay_left_rows={};replay_right_rows={};rows_emitted=0",
            left_rows.len(),
            right_rows.len()
        );
        env.cassie.runtime.record_runtime_operator_switch(
            VECTOR_TO_MERGE_SWITCH_PAIR,
            "row_threshold_exceeded",
            &state,
        );
        return Ok(None);
    }

    let estimated_bytes = estimate_vectorized_join_bytes(left_rows.len(), right_rows.len());
    if estimated_bytes > env.controls.temp_spill_budget_bytes {
        if limits.operator_switching_enabled.is_enabled() {
            env.cassie.runtime.record_runtime_operator_switch_fallback(
                VECTOR_TO_MERGE_SWITCH_PAIR,
                "spill_budget_exceeded",
                "rows_emitted=0",
            );
        }
        env.cassie.runtime.record_vectorized_join_fallback(
            "spill_budget_exceeded",
            batch_size,
            true,
        );
        return Ok(None);
    }

    Ok(Some(batch_size))
}

struct MergeEqualGroupsResult {
    joined: Vec<BatchRow>,
    matched_rows: usize,
}

fn merge_equal_key_groups(
    env: &SourceExecutionEnv<'_>,
    kind: JoinKind,
    on: &Expr,
    left_group: &[KeyedRow],
    right_group: &[KeyedRow],
    left_columns: &[String],
    right_columns: &[String],
) -> Result<MergeEqualGroupsResult, QueryError> {
    let mut left_matched = vec![false; left_group.len()];
    let mut right_matched = vec![false; right_group.len()];
    let mut joined = Vec::new();
    let mut matched_rows = 0usize;

    for (left_offset, left) in left_group.iter().enumerate() {
        for (right_offset, right) in right_group.iter().enumerate() {
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
                    &left_group[offset].row,
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
                    &right_group[offset].row,
                ));
            }
        }
    }

    Ok(MergeEqualGroupsResult {
        joined,
        matched_rows,
    })
}

fn append_left_unmatched(
    joined: &mut Vec<BatchRow>,
    kind: JoinKind,
    rows: &[KeyedRow],
    right_columns: &[String],
) {
    if matches!(kind, JoinKind::Left | JoinKind::Full) {
        for left in rows {
            joined.push(combine_row_with_nulls(&left.row, right_columns));
        }
    }
}

fn append_right_unmatched(
    joined: &mut Vec<BatchRow>,
    kind: JoinKind,
    left_columns: &[String],
    rows: &[KeyedRow],
) {
    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        for right in rows {
            joined.push(combine_nulls_with_row(left_columns, &right.row));
        }
    }
}
