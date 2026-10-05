use super::super::{accounting, JoinResult, JoinRows, PendingJoinDiagnostic};
use super::{
    can_dense_stream, check_timeout, collection_scan_fields, combine_rows, filter,
    is_temp_budget_error, load_collection_rows, project_source_row, reserve_probe_key,
    row_join_key, BatchRow, JoinRetentionContext, JoinRetentionPhase, QueryError,
    SourceExecutionEnv, SourceRowShape, StreamingJoinSpec,
};
use crate::executor::semantic::SemanticKey;
use crate::runtime::accounted::AccountedVec;

pub(super) fn execute_left_build_streaming_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let left_rows = load_collection_rows(env, spec.left_collection)?;
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    if left_rows.is_empty() {
        return empty_vectorized_join(env, batch_size);
    }

    retention.enter(JoinRetentionPhase::BoundedBuild);
    let _build_memory =
        env.controls
            .reserve_query_memory(accounting::hash_build_bytes::<usize>(
                left_rows.as_slice(),
                &spec.keys.left,
            )?)?;
    retention.before(JoinRetentionPhase::BoundedBuild)?;
    let mut build = std::collections::HashMap::<SemanticKey, Vec<usize>>::new();
    build
        .try_reserve(accounting::hash_build_capacity(
            left_rows.as_slice(),
            &spec.keys.left,
        ))
        .map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to retain bounded join build: {error}"
            ))
        })?;
    for (index, left_row) in left_rows.as_slice().iter().enumerate() {
        check_timeout(env.controls)?;
        if let Some(key) = row_join_key(left_row, &spec.keys.left) {
            build.entry(key).or_default().push(index);
        }
    }

    let progress = stream_rows(
        env,
        spec,
        &StreamBuild {
            rows: left_rows.as_slice(),
            index: &build,
            left: true,
        },
        retention,
    )?;
    env.cassie.runtime.record_read_path_collection_scan(
        spec.right_collection,
        spec.right_scan_fields.len(),
        progress.scanned,
    );
    Ok(JoinResult::new(
        progress.joined,
        PendingJoinDiagnostic::VectorizedRoles {
            left_rows: left_rows.len(),
            right_rows: progress.probe_rows,
            build_rows: left_rows.len(),
            probe_rows: progress.probe_rows,
            matched_rows: progress.matched_rows,
            batch_size,
            batches: progress.probe_rows.div_ceil(batch_size),
        },
    ))
}

pub(super) fn execute_dense_streaming_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    let Some(right_scan_fields) = collection_scan_fields(env, spec.right_collection) else {
        return empty_vectorized_join(env, batch_size);
    };
    let left_schema = env.cassie.catalog.get_schema(spec.left_collection);
    let right_schema = env.cassie.catalog.get_schema(spec.right_collection);
    let left_shape = SourceRowShape {
        collection: spec.left_collection,
        fields: &spec.left_scan_fields,
        schema: left_schema.as_ref(),
    };
    let right_shape = SourceRowShape {
        collection: spec.right_collection,
        fields: &right_scan_fields,
        schema: right_schema.as_ref(),
    };
    let mut joined = JoinRows::try_new(env.controls)?;
    let mut probe_rows = 0usize;
    let mut build_rows = 0usize;
    let mut matched_rows = 0usize;
    let mut right_scanned = 0usize;

    let left_scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
        spec.left_collection,
        crate::midge::adapter::RowDecode::Full,
        env.controls,
        |left_document| {
            let left_row = project_source_row(
                env,
                &left_document,
                left_shape,
                JoinRetentionPhase::BoundedSource,
                retention,
            )?;
            probe_rows += 1;
            let _left_key_memory = reserve_probe_key(env, &left_row, &spec.keys.left)?;
            let Some(left_key) = row_join_key(&left_row, &spec.keys.left) else {
                return Ok(true);
            };

            let scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
                spec.right_collection,
                crate::midge::adapter::RowDecode::Full,
                env.controls,
                |right_document| {
                    let right_row = project_source_row(
                        env,
                        &right_document,
                        right_shape,
                        JoinRetentionPhase::BoundedSource,
                        retention,
                    )?;
                    build_rows += 1;
                    let _right_key_memory = reserve_probe_key(env, &right_row, &spec.keys.right)?;
                    let Some(right_key) = row_join_key(&right_row, &spec.keys.right) else {
                        return Ok(true);
                    };
                    if left_key != right_key {
                        return Ok(true);
                    }
                    if append_matching_row(
                        env,
                        spec,
                        &left_row,
                        &right_row,
                        &mut joined,
                        retention,
                    )? {
                        matched_rows += 1;
                    }
                    Ok(joined.len() < spec.output_budget)
                },
            )?;
            right_scanned += scanned;
            Ok(joined.len() < spec.output_budget)
        },
    )?;
    record_dense_source_reads(
        env,
        spec,
        left_scanned,
        right_scan_fields.len(),
        right_scanned,
    );
    check_timeout(env.controls)?;
    Ok(JoinResult::new(
        joined,
        PendingJoinDiagnostic::Vectorized {
            probe_rows,
            build_rows,
            matched_rows,
            batch_size,
            batches: probe_rows,
        },
    ))
}

fn record_dense_source_reads(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    left_scanned: usize,
    right_field_count: usize,
    right_scanned: usize,
) {
    env.cassie.runtime.record_read_path_collection_scan(
        spec.left_collection,
        spec.left_scan_fields.len(),
        left_scanned,
    );
    env.cassie.runtime.record_read_path_collection_scan(
        spec.right_collection,
        right_field_count,
        right_scanned,
    );
}

fn empty_vectorized_join(
    env: &SourceExecutionEnv<'_>,
    batch_size: usize,
) -> Result<JoinResult, QueryError> {
    Ok(JoinResult::new(
        JoinRows::try_new(env.controls)?,
        PendingJoinDiagnostic::Vectorized {
            probe_rows: 0,
            build_rows: 0,
            matched_rows: 0,
            batch_size,
            batches: 0,
        },
    ))
}

pub(super) enum StreamingRightRows {
    Rows(AccountedVec<BatchRow>),
    Dense(JoinResult),
}

pub(super) fn load_streaming_right_rows(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<StreamingRightRows, QueryError> {
    match load_collection_rows(env, spec.right_collection) {
        Ok(rows) => Ok(StreamingRightRows::Rows(rows)),
        Err(error)
            if is_temp_budget_error(&error)
                && can_dense_stream(env, spec.left_collection, spec.right_collection)? =>
        {
            execute_dense_streaming_bounded_inner_join(env, spec, retention)
                .map(StreamingRightRows::Dense)
        }
        Err(error) => Err(error),
    }
}

pub(super) struct StreamingJoinProgress {
    pub(super) joined: JoinRows,
    pub(super) probe_rows: usize,
    pub(super) matched_rows: usize,
    pub(super) scanned: usize,
}

pub(super) fn stream_left_rows_against_right(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    build: &std::collections::HashMap<SemanticKey, Vec<usize>>,
    right_rows: &[BatchRow],
    retention: &JoinRetentionContext<'_>,
) -> Result<StreamingJoinProgress, QueryError> {
    stream_rows(
        env,
        spec,
        &StreamBuild {
            rows: right_rows,
            index: build,
            left: false,
        },
        retention,
    )
}

struct StreamBuild<'a> {
    rows: &'a [BatchRow],
    index: &'a std::collections::HashMap<SemanticKey, Vec<usize>>,
    left: bool,
}

fn stream_rows(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    build: &StreamBuild<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<StreamingJoinProgress, QueryError> {
    let (collection, fields, column) = if build.left {
        (
            spec.right_collection,
            &spec.right_scan_fields,
            &spec.keys.right,
        )
    } else {
        (
            spec.left_collection,
            &spec.left_scan_fields,
            &spec.keys.left,
        )
    };
    let schema = env.cassie.catalog.get_schema(collection);
    let shape = SourceRowShape {
        collection,
        fields,
        schema: schema.as_ref(),
    };
    let mut joined = JoinRows::try_new(env.controls)?;
    let mut probe_rows = 0usize;
    let mut matched_rows = 0usize;
    let scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
        collection,
        crate::midge::adapter::RowDecode::Full,
        env.controls,
        |document| {
            let row = project_source_row(
                env,
                &document,
                shape,
                JoinRetentionPhase::BoundedSource,
                retention,
            )?;
            probe_rows += 1;
            let _key_memory = reserve_probe_key(env, &row, column)?;
            let Some(key) = row_join_key(&row, column) else {
                return Ok(true);
            };
            if let Some(indexes) = build.index.get(&key) {
                for index in indexes {
                    check_timeout(env.controls)?;
                    let build_row = &build.rows[*index];
                    let (left, right) = if build.left {
                        (build_row, &row)
                    } else {
                        (&row, build_row)
                    };
                    if append_matching_row(env, spec, left, right, &mut joined, retention)? {
                        matched_rows += 1;
                    }
                    if joined.len() >= spec.output_budget {
                        return Ok(false);
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

fn append_matching_row(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    left: &BatchRow,
    right: &BatchRow,
    joined: &mut JoinRows,
    retention: &JoinRetentionContext<'_>,
) -> Result<bool, QueryError> {
    retention.enter(JoinRetentionPhase::BoundedOutput);
    joined.try_push_combined(left, right, || {
        retention.before(JoinRetentionPhase::BoundedOutput)?;
        let combined = combine_rows(left, right);
        Ok(filter::eval_scalar(
            &combined,
            spec.on,
            env.params,
            None,
            env.user_functions,
            None,
            env.session,
        )?
        .is_true()?
        .then_some(combined))
    })
}
