use super::{
    batch, catalog, check_timeout, collection_join_columns, collection_scan_fields, combine_rows,
    estimate_vectorized_join_bytes, execute_query_source, filter, join_field_for_collection,
    merge_join_keys, qualify_row, row_join_key, scan, BatchRow, CteContext, EquiJoinKeys, Expr,
    JoinExecutionSpec, JoinKind, QueryError, QuerySource, SourceExecutionEnv, Value,
};

#[path = "bounded/side_selection.rs"]
mod side_selection;

struct StreamingJoinSpec<'a> {
    left_collection: &'a str,
    right_collection: &'a str,
    on: &'a Expr,
    keys: EquiJoinKeys,
    left_scan_fields: Vec<String>,
    right_scan_fields: Vec<String>,
    output_budget: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JoinSide {
    Left,
    Right,
}

const ROW_COUNT_BUILD_SIDE_RATIO: u64 = 4;

struct IndexedJoinPlan {
    indexed_side: JoinSide,
    index: catalog::IndexMeta,
    indexed_scan_fields: Vec<String>,
    stream_scan_fields: Vec<String>,
}

pub(super) fn try_execute_indexed_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &JoinExecutionSpec<'_>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(output_budget) = spec.row_budget else {
        return Ok(None);
    };
    if output_budget == 0 {
        return Ok(Some(Vec::new()));
    }
    let limits = env.cassie.runtime.limits();
    if !limits.vectorized_joins_enabled || !matches!(spec.kind, JoinKind::Inner) {
        return Ok(None);
    }

    let (QuerySource::Collection(left_collection), QuerySource::Collection(right_collection)) =
        (spec.left, spec.right)
    else {
        return Ok(None);
    };
    let Some(left_columns) = collection_join_columns(env, left_collection) else {
        return Ok(None);
    };
    let Some(right_columns) = collection_join_columns(env, right_collection) else {
        return Ok(None);
    };
    let Some(keys) = merge_join_keys(spec.on, &left_columns, &right_columns) else {
        return Ok(None);
    };
    let Some(plan) = indexed_join_plan(env, left_collection, right_collection, &keys) else {
        return Ok(None);
    };

    execute_indexed_bounded_inner_join(
        env,
        left_collection,
        right_collection,
        spec.on,
        &keys,
        &plan,
        output_budget,
    )
    .map(Some)
}

fn execute_indexed_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    right_collection: &str,
    on: &Expr,
    keys: &EquiJoinKeys,
    plan: &IndexedJoinPlan,
    output_budget: usize,
) -> Result<Vec<BatchRow>, QueryError> {
    let stream_collection = match plan.indexed_side {
        JoinSide::Left => right_collection,
        JoinSide::Right => left_collection,
    };
    let stream_key = match plan.indexed_side {
        JoinSide::Left => &keys.right,
        JoinSide::Right => &keys.left,
    };
    let stream_schema = env.cassie.catalog.get_schema(stream_collection);
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    let mut joined = Vec::with_capacity(output_budget.min(batch_size));
    let mut left_input_rows = 0usize;
    let mut right_input_rows = 0usize;
    let mut matched_rows = 0usize;
    let mut index_scans = 0usize;

    let streamed_rows = env.cassie.midge.scan_rows_until::<QueryError, _>(
        stream_collection,
        crate::midge::adapter::RowDecode::Full,
        |document| {
            check_timeout(env.controls)?;
            let stream_row = qualify_row(
                scan::projected_document_to_row(
                    document,
                    &plan.stream_scan_fields,
                    stream_schema.as_ref(),
                ),
                stream_collection,
            );
            match plan.indexed_side {
                JoinSide::Left => right_input_rows += 1,
                JoinSide::Right => left_input_rows += 1,
            }
            let Some(key_value) = stream_row.get(stream_key).and_then(value_to_json) else {
                return Ok(true);
            };
            let remaining = output_budget.saturating_sub(joined.len());
            if remaining == 0 {
                return Ok(false);
            }
            let indexed_rows = scan_indexed_join_rows(
                env,
                match plan.indexed_side {
                    JoinSide::Left => left_collection,
                    JoinSide::Right => right_collection,
                },
                &plan.indexed_scan_fields,
                &plan.index,
                key_value,
                remaining,
            )?;
            index_scans += 1;

            for indexed_row in indexed_rows {
                match plan.indexed_side {
                    JoinSide::Left => left_input_rows += 1,
                    JoinSide::Right => right_input_rows += 1,
                }
                let combined = match plan.indexed_side {
                    JoinSide::Left => combine_rows(&indexed_row, &stream_row),
                    JoinSide::Right => combine_rows(&stream_row, &indexed_row),
                };
                if filter::eval_scalar(
                    &combined,
                    on,
                    env.params,
                    None,
                    env.user_functions,
                    None,
                    env.session,
                )?
                .is_true()
                {
                    matched_rows += 1;
                    joined.push(combined);
                    if joined.len() >= output_budget {
                        return Ok(false);
                    }
                }
            }

            Ok(true)
        },
    )?;

    env.cassie.runtime.record_read_path_collection_scan(
        stream_collection,
        plan.stream_scan_fields.len(),
        streamed_rows,
    );
    env.cassie.runtime.record_vectorized_join_execution(
        left_input_rows,
        right_input_rows,
        matched_rows,
        joined.len(),
        batch_size,
        index_scans,
    );
    Ok(joined)
}

fn indexed_join_plan(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    right_collection: &str,
    keys: &EquiJoinKeys,
) -> Option<IndexedJoinPlan> {
    let left_field = join_field_for_collection(&keys.left, left_collection)?;
    let right_field = join_field_for_collection(&keys.right, right_collection)?;
    let left_index = usable_scalar_join_index(env, left_collection, &left_field);
    let right_index = usable_scalar_join_index(env, right_collection, &right_field);
    let indexed_side = match (&left_index, &right_index) {
        (Some(_), Some(_)) => indexed_side_for_dual_indexes(env, left_collection, right_collection),
        (Some(_), None) => JoinSide::Left,
        (None, Some(_)) => JoinSide::Right,
        (None, None) => return None,
    };
    let stream_collection = match indexed_side {
        JoinSide::Left => right_collection,
        JoinSide::Right => left_collection,
    };
    if has_session_changes(env, stream_collection) {
        return None;
    }
    let (index, indexed_collection) = match indexed_side {
        JoinSide::Left => (left_index?, left_collection),
        JoinSide::Right => (right_index?, right_collection),
    };
    Some(IndexedJoinPlan {
        indexed_side,
        index,
        indexed_scan_fields: collection_scan_fields(env, indexed_collection)?,
        stream_scan_fields: collection_scan_fields(env, stream_collection)?,
    })
}

fn indexed_side_for_dual_indexes(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    right_collection: &str,
) -> JoinSide {
    match (
        hydrated_row_count(env, left_collection),
        hydrated_row_count(env, right_collection),
    ) {
        (Some(left_rows), Some(right_rows)) if left_rows < right_rows => JoinSide::Right,
        _ => JoinSide::Left,
    }
}

pub(super) fn try_execute_streaming_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    join: &JoinExecutionSpec<'_>,
    cte_context: &mut CteContext,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if join.row_budget == Some(0) {
        return Ok(Some(Vec::new()));
    }
    let Some(spec) = streaming_join_spec(
        env,
        join.left,
        join.right,
        join.kind,
        join.on,
        join.row_budget,
    ) else {
        return Ok(None);
    };
    let limits = env.cassie.runtime.limits();
    if should_preemptively_dense_stream(env, spec.left_collection, spec.right_collection)? {
        env.cassie
            .runtime
            .record_bounded_join_side_selection("dense_stream_preemptive_temp_budget");
        return execute_dense_streaming_bounded_inner_join(env, &spec).map(Some);
    }
    let side_selection = side_selection::build_side_for_streaming(env, &spec)?;
    env.cassie
        .runtime
        .record_bounded_join_side_selection(side_selection.reason);
    if side_selection.build_left {
        return execute_left_build_streaming_bounded_inner_join(env, &spec).map(Some);
    }

    let right_rows =
        match load_streaming_right_rows(env, join.right, cte_context, join.outer_row, &spec)? {
            StreamingRightRows::Rows(right_rows) => right_rows,
            StreamingRightRows::Dense(rows) => return Ok(Some(rows)),
        };
    if right_rows.is_empty() {
        env.cassie.runtime.record_vectorized_join_execution(
            0,
            0,
            0,
            0,
            limits.vectorized_join_batch_size.max(1),
            0,
        );
        return Ok(Some(Vec::new()));
    }

    let mut build = std::collections::HashMap::<String, Vec<usize>>::new();
    for (index, right_row) in right_rows.iter().enumerate() {
        if let Some(key) = row_join_key(right_row, &spec.keys.right) {
            build.entry(key).or_default().push(index);
        }
    }

    let batch_size = limits.vectorized_join_batch_size.max(1);
    let progress = stream_left_rows_against_right(env, &spec, &build, &right_rows, batch_size)?;
    env.cassie.runtime.record_read_path_collection_scan(
        spec.left_collection,
        spec.left_scan_fields.len(),
        progress.scanned,
    );
    env.cassie.runtime.record_vectorized_join_execution(
        progress.probe_rows,
        right_rows.len(),
        progress.matched_rows,
        progress.joined.len(),
        batch_size,
        progress.probe_rows.div_ceil(batch_size),
    );
    Ok(Some(progress.joined))
}

fn execute_left_build_streaming_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
) -> Result<Vec<BatchRow>, QueryError> {
    let left_rows = load_collection_rows(env, spec.left_collection)?;
    if left_rows.is_empty() {
        record_empty_vectorized_join(env);
        return Ok(Vec::new());
    }

    let mut build = std::collections::HashMap::<String, Vec<usize>>::new();
    for (index, left_row) in left_rows.iter().enumerate() {
        if let Some(key) = row_join_key(left_row, &spec.keys.left) {
            build.entry(key).or_default().push(index);
        }
    }

    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    let progress = stream_right_rows_against_left(env, spec, &build, &left_rows, batch_size)?;
    env.cassie.runtime.record_read_path_collection_scan(
        spec.right_collection,
        spec.right_scan_fields.len(),
        progress.scanned,
    );
    env.cassie
        .runtime
        .record_vectorized_join_execution_with_roles(
            crate::runtime::VectorizedJoinInputRows {
                left: left_rows.len(),
                right: progress.probe_rows,
                build: left_rows.len(),
                probe: progress.probe_rows,
            },
            progress.matched_rows,
            progress.joined.len(),
            batch_size,
            progress.probe_rows.div_ceil(batch_size),
        );
    Ok(progress.joined)
}

fn execute_dense_streaming_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
) -> Result<Vec<BatchRow>, QueryError> {
    let Some(right_scan_fields) = collection_scan_fields(env, spec.right_collection) else {
        return Ok(Vec::new());
    };
    let left_schema = env.cassie.catalog.get_schema(spec.left_collection);
    let right_schema = env.cassie.catalog.get_schema(spec.right_collection);
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    let mut joined = Vec::with_capacity(spec.output_budget.min(batch_size));
    let mut probe_rows = 0usize;
    let mut build_rows = 0usize;
    let mut matched_rows = 0usize;
    let mut right_scanned = 0usize;

    let left_scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
        spec.left_collection,
        crate::midge::adapter::RowDecode::Full,
        |left_document| {
            check_timeout(env.controls)?;
            let left_row = qualify_row(
                scan::projected_document_to_row(
                    left_document,
                    &spec.left_scan_fields,
                    left_schema.as_ref(),
                ),
                spec.left_collection,
            );
            probe_rows += 1;
            let Some(left_key) = row_join_key(&left_row, &spec.keys.left) else {
                return Ok(true);
            };

            let scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
                spec.right_collection,
                crate::midge::adapter::RowDecode::Full,
                |right_document| {
                    check_timeout(env.controls)?;
                    let right_row = qualify_row(
                        scan::projected_document_to_row(
                            right_document,
                            &right_scan_fields,
                            right_schema.as_ref(),
                        ),
                        spec.right_collection,
                    );
                    build_rows += 1;
                    let Some(right_key) = row_join_key(&right_row, &spec.keys.right) else {
                        return Ok(true);
                    };
                    if left_key != right_key {
                        return Ok(true);
                    }

                    let combined = combine_rows(&left_row, &right_row);
                    if filter::eval_scalar(
                        &combined,
                        spec.on,
                        env.params,
                        None,
                        env.user_functions,
                        None,
                        env.session,
                    )?
                    .is_true()
                    {
                        matched_rows += 1;
                        joined.push(combined);
                        if joined.len() >= spec.output_budget {
                            return Ok(false);
                        }
                    }

                    Ok(true)
                },
            )?;
            right_scanned += scanned;
            Ok(joined.len() < spec.output_budget)
        },
    )?;

    env.cassie.runtime.record_read_path_collection_scan(
        spec.left_collection,
        spec.left_scan_fields.len(),
        left_scanned,
    );
    env.cassie.runtime.record_read_path_collection_scan(
        spec.right_collection,
        right_scan_fields.len(),
        right_scanned,
    );
    env.cassie.runtime.record_vectorized_join_execution(
        probe_rows,
        build_rows,
        matched_rows,
        joined.len(),
        batch_size,
        probe_rows,
    );
    Ok(joined)
}

fn load_collection_rows(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let batches = scan::scan_limit(env.cassie, env.session, collection, None)?;
    Ok(batch::flatten_batches(batches)
        .into_iter()
        .map(|row| qualify_row(row, collection))
        .collect())
}

fn record_empty_vectorized_join(env: &SourceExecutionEnv<'_>) {
    env.cassie.runtime.record_vectorized_join_execution(
        0,
        0,
        0,
        0,
        env.cassie
            .runtime
            .limits()
            .vectorized_join_batch_size
            .max(1),
        0,
    );
}

enum StreamingRightRows {
    Rows(Vec<BatchRow>),
    Dense(Vec<BatchRow>),
}

fn load_streaming_right_rows(
    env: &SourceExecutionEnv<'_>,
    right: &QuerySource,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
    spec: &StreamingJoinSpec<'_>,
) -> Result<StreamingRightRows, QueryError> {
    match execute_query_source(env, right, cte_context, true, outer_row, None) {
        Ok((right_batches, _right_text)) => Ok(StreamingRightRows::Rows(batch::flatten_batches(
            right_batches,
        ))),
        Err(error)
            if is_temp_budget_error(&error)
                && can_dense_stream(env, spec.left_collection, spec.right_collection)? =>
        {
            execute_dense_streaming_bounded_inner_join(env, spec).map(StreamingRightRows::Dense)
        }
        Err(error) => Err(error),
    }
}

struct StreamingJoinProgress {
    joined: Vec<BatchRow>,
    probe_rows: usize,
    matched_rows: usize,
    scanned: usize,
}

fn stream_left_rows_against_right(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    build: &std::collections::HashMap<String, Vec<usize>>,
    right_rows: &[BatchRow],
    batch_size: usize,
) -> Result<StreamingJoinProgress, QueryError> {
    let schema = env.cassie.catalog.get_schema(spec.left_collection);
    let mut joined = Vec::with_capacity(spec.output_budget.min(batch_size));
    let mut probe_rows = 0usize;
    let mut matched_rows = 0usize;

    let scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
        spec.left_collection,
        crate::midge::adapter::RowDecode::Full,
        |document| {
            check_timeout(env.controls)?;
            let left_row = qualify_row(
                scan::projected_document_to_row(document, &spec.left_scan_fields, schema.as_ref()),
                spec.left_collection,
            );
            probe_rows += 1;
            let Some(key) = row_join_key(&left_row, &spec.keys.left) else {
                return Ok(true);
            };

            if let Some(right_indexes) = build.get(&key) {
                for right_index in right_indexes {
                    let combined = combine_rows(&left_row, &right_rows[*right_index]);
                    if filter::eval_scalar(
                        &combined,
                        spec.on,
                        env.params,
                        None,
                        env.user_functions,
                        None,
                        env.session,
                    )?
                    .is_true()
                    {
                        matched_rows += 1;
                        joined.push(combined);
                        if joined.len() >= spec.output_budget {
                            return Ok(false);
                        }
                    }
                }
            }

            Ok(true)
        },
    )?;

    Ok(StreamingJoinProgress {
        joined,
        probe_rows,
        matched_rows,
        scanned,
    })
}

fn stream_right_rows_against_left(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    build: &std::collections::HashMap<String, Vec<usize>>,
    left_rows: &[BatchRow],
    batch_size: usize,
) -> Result<StreamingJoinProgress, QueryError> {
    let schema = env.cassie.catalog.get_schema(spec.right_collection);
    let mut joined = Vec::with_capacity(spec.output_budget.min(batch_size));
    let mut probe_rows = 0usize;
    let mut matched_rows = 0usize;

    let scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
        spec.right_collection,
        crate::midge::adapter::RowDecode::Full,
        |document| {
            check_timeout(env.controls)?;
            let right_row = qualify_row(
                scan::projected_document_to_row(document, &spec.right_scan_fields, schema.as_ref()),
                spec.right_collection,
            );
            probe_rows += 1;
            let Some(key) = row_join_key(&right_row, &spec.keys.right) else {
                return Ok(true);
            };

            if let Some(left_indexes) = build.get(&key) {
                for left_index in left_indexes {
                    let combined = combine_rows(&left_rows[*left_index], &right_row);
                    if filter::eval_scalar(
                        &combined,
                        spec.on,
                        env.params,
                        None,
                        env.user_functions,
                        None,
                        env.session,
                    )?
                    .is_true()
                    {
                        matched_rows += 1;
                        joined.push(combined);
                        if joined.len() >= spec.output_budget {
                            return Ok(false);
                        }
                    }
                }
            }

            Ok(true)
        },
    )?;

    Ok(StreamingJoinProgress {
        joined,
        probe_rows,
        matched_rows,
        scanned,
    })
}

fn streaming_join_spec<'a>(
    env: &SourceExecutionEnv<'_>,
    left: &'a QuerySource,
    right: &'a QuerySource,
    kind: JoinKind,
    on: &'a Expr,
    row_budget: Option<usize>,
) -> Option<StreamingJoinSpec<'a>> {
    let output_budget = row_budget?;

    let limits = env.cassie.runtime.limits();
    if !limits.vectorized_joins_enabled || !matches!(kind, JoinKind::Inner) {
        return None;
    }

    let (QuerySource::Collection(left_collection), QuerySource::Collection(right_collection)) =
        (left, right)
    else {
        return None;
    };
    if has_session_changes(env, left_collection) {
        return None;
    }

    let left_columns = collection_join_columns(env, left_collection)?;
    let right_columns = collection_join_columns(env, right_collection)?;
    let keys = merge_join_keys(on, &left_columns, &right_columns)?;
    let left_field = join_field_for_collection(&keys.left, left_collection)?;
    if usable_scalar_join_index(env, left_collection, &left_field).is_some() {
        return None;
    }
    let right_field = join_field_for_collection(&keys.right, right_collection)?;
    if usable_scalar_join_index(env, right_collection, &right_field).is_some() {
        return None;
    }
    let left_scan_fields = collection_scan_fields(env, left_collection)?;
    let right_scan_fields = collection_scan_fields(env, right_collection)?;

    Some(StreamingJoinSpec {
        left_collection,
        right_collection,
        on,
        keys,
        left_scan_fields,
        right_scan_fields,
        output_budget,
    })
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

fn usable_scalar_join_index(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    field: &str,
) -> Option<catalog::IndexMeta> {
    (!has_session_changes(env, collection))
        .then(|| scalar_join_index(env, collection, field))
        .flatten()
}

fn hydrated_row_count(env: &SourceExecutionEnv<'_>, collection: &str) -> Option<u64> {
    env.cassie
        .catalog
        .get_cardinality_stats(collection)
        .filter(|stats| stats.hydrated)
        .map(|stats| stats.row_count)
}

fn scan_indexed_join_rows(
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
            &crate::midge::adapter::ScalarIndexScanRequest {
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

fn has_session_changes(env: &SourceExecutionEnv<'_>, collection: &str) -> bool {
    env.session
        .is_some_and(|session| !session.collection_changes(collection).is_empty())
}

fn can_dense_stream(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    right_collection: &str,
) -> Result<bool, QueryError> {
    if has_session_changes(env, right_collection) {
        return Ok(false);
    }
    let left_column_store = env
        .cassie
        .midge
        .collection_uses_column_store(left_collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let right_column_store = env
        .cassie
        .midge
        .collection_uses_column_store(right_collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(!left_column_store && !right_column_store)
}

fn should_preemptively_dense_stream(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    right_collection: &str,
) -> Result<bool, QueryError> {
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    let estimated_batch_bytes = estimate_vectorized_join_bytes(batch_size, batch_size);
    Ok(
        env.controls.temp_spill_budget_bytes <= estimated_batch_bytes
            && can_dense_stream(env, left_collection, right_collection)?,
    )
}

fn is_temp_budget_error(error: &QueryError) -> bool {
    matches!(
        error,
        QueryError::General(message)
            if message.starts_with("temporary storage budget exceeded:")
    )
}
