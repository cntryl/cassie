use super::*;

const VECTOR_TO_MERGE_SWITCH_PAIR: &str = "vectorized_join_to_merge_join";

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
    row_budget: Option<usize>,
) -> SourceExecution<'a> {
    if !source_contains_lateral(right) {
        if let Some(joined) = try_execute_indexed_bounded_inner_join(
            env,
            left,
            right,
            kind,
            on,
            cte_context,
            outer_row,
            row_budget,
        )? {
            return finish_join(joined);
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
            keys.clone(),
            &left_rows,
            &right_rows,
            &right_columns,
            row_budget,
        )?
        .map(Ok)
        .unwrap_or_else(|| {
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
        })?,
        None => execute_nested_loop_join(
            env,
            kind,
            on,
            left_rows,
            right_rows,
            &left_columns,
            &right_columns,
        )?,
    };

    finish_join(joined)
}

#[allow(clippy::too_many_arguments)]
fn try_execute_indexed_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    left: &QuerySource,
    right: &QuerySource,
    kind: &JoinKind,
    on: &Expr,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
    row_budget: Option<usize>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let output_budget = row_budget.unwrap_or(usize::MAX);
    if output_budget == 0 {
        return Ok(Some(Vec::new()));
    }
    let limits = env.cassie.runtime.limits();
    if !limits.vectorized_joins_enabled || !matches!(kind, JoinKind::Inner) {
        return Ok(None);
    }

    let (QuerySource::Collection(left_collection), QuerySource::Collection(right_collection)) =
        (left, right)
    else {
        return Ok(None);
    };
    if env
        .session
        .map(|session| !session.collection_changes(left_collection).is_empty())
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let Some(left_columns) = collection_join_columns(env, left_collection) else {
        return Ok(None);
    };
    let Some(right_columns) = collection_join_columns(env, right_collection) else {
        return Ok(None);
    };
    let Some(keys) = merge_join_keys(on, &left_columns, &right_columns) else {
        return Ok(None);
    };
    let Some(left_field) = join_field_for_collection(&keys.left, left_collection) else {
        return Ok(None);
    };
    let Some(index) = scalar_join_index(env, left_collection, &left_field) else {
        return Ok(None);
    };
    let Some(left_scan_fields) = collection_scan_fields(env, left_collection) else {
        return Ok(None);
    };

    let (right_batches, _right_text) =
        execute_query_source(env, right, cte_context, true, outer_row, None)?;
    let right_rows = batch::flatten_batches(right_batches);
    let batch_size = limits.vectorized_join_batch_size.max(1);
    let mut joined = Vec::with_capacity(output_budget.min(batch_size));
    let mut probe_rows = 0usize;
    let mut matched_rows = 0usize;
    let mut index_scans = 0usize;

    'right: for right_row in &right_rows {
        check_timeout(env.controls)?;
        let Some(key_value) = right_row.get(&keys.right).and_then(value_to_json) else {
            continue;
        };
        let remaining = output_budget.saturating_sub(joined.len());
        if remaining == 0 {
            break;
        }
        let left_rows = scan_indexed_left_rows(
            env,
            left_collection,
            &left_scan_fields,
            &index,
            key_value,
            remaining,
        )?;
        index_scans += 1;
        probe_rows += left_rows.len();

        for left_row in left_rows {
            let combined = combine_rows(&left_row, right_row);
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
                matched_rows += 1;
                joined.push(combined);
                if joined.len() >= output_budget {
                    break 'right;
                }
            }
        }
    }

    env.cassie.runtime.record_vectorized_join_execution(
        probe_rows,
        right_rows.len(),
        matched_rows,
        joined.len(),
        batch_size,
        index_scans.max(1),
    );
    Ok(Some(joined))
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

fn scalar_join_index(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    field: &str,
) -> Option<catalog::IndexMeta> {
    env.cassie
        .catalog
        .list_indexes(collection)
        .into_iter()
        .find(|index| {
            index.kind == catalog::IndexKind::Scalar
                && index.predicate.is_none()
                && index.normalized_expressions().is_empty()
                && index
                    .normalized_fields()
                    .first()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(field))
        })
}

fn scan_indexed_left_rows(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    scan_fields: &[String],
    index: &catalog::IndexMeta,
    key_value: serde_json::Value,
    limit: usize,
) -> Result<Vec<BatchRow>, QueryError> {
    let hits = env
        .cassie
        .midge
        .scan_scalar_index(
            index,
            crate::midge::adapter::ScalarIndexScanRequest {
                equality_prefix: vec![key_value],
                limit: Some(limit),
                ..Default::default()
            },
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    env.cassie
        .runtime
        .record_read_path_index_seek(collection, hits.len(), &index.name);

    let schema = env.cassie.catalog.get_schema(collection);
    let mut rows = Vec::with_capacity(hits.len());
    for hit in hits {
        let Some(document) = env
            .cassie
            .get_document_for_session(env.session, collection, &hit.id)
            .map_err(|error| QueryError::General(error.to_string()))?
        else {
            continue;
        };
        rows.push(qualify_row(
            scan::projected_document_to_row(document, scan_fields, schema.as_ref()),
            collection,
        ));
    }
    Ok(rows)
}

fn value_to_json(value: &Value) -> Option<serde_json::Value> {
    match value {
        Value::Null => Some(serde_json::Value::Null),
        Value::Bool(value) => Some(serde_json::Value::Bool(*value)),
        Value::Int64(value) => Some(serde_json::Value::Number((*value).into())),
        Value::Float64(value) => {
            serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
        }
        Value::String(value) => Some(serde_json::Value::String(value.clone())),
        Value::Vector(_) | Value::Json(_) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_vectorized_join(
    env: &SourceExecutionEnv<'_>,
    kind: &JoinKind,
    _on: &Expr,
    keys: EquiJoinKeys,
    left_rows: &[BatchRow],
    right_rows: &[BatchRow],
    right_columns: &[String],
    row_budget: Option<usize>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let limits = env.cassie.runtime.limits();
    let batch_size = limits.vectorized_join_batch_size.max(1);
    if !limits.vectorized_joins_enabled {
        return Ok(None);
    }
    if !matches!(kind, JoinKind::Inner | JoinKind::Left) {
        if limits.operator_switching_enabled {
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
    if limits.operator_switching_enabled
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
        if limits.operator_switching_enabled {
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
        .map(|row| {
            let key = row
                .get(key_column)
                .map(value_sort_key)
                .unwrap_or_else(|| value_sort_key(&Value::Null));
            KeyedRow { key, row }
        })
        .collect()
}

fn row_join_key(row: &BatchRow, key_column: &str) -> String {
    row.get(key_column)
        .map(value_sort_key)
        .unwrap_or_else(|| value_sort_key(&Value::Null))
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
