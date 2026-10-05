use super::{
    accounting, catalog, check_timeout, collection_join_columns, collection_scan_fields,
    combine_rows, estimate_vectorized_join_bytes, filter, join_field_for_collection,
    merge_join_keys, qualify_row, row_join_key, scan, BatchRow, CteContext, EquiJoinKeys, Expr,
    JoinExecutionSpec, JoinKind, JoinResult, JoinRetentionContext, JoinRetentionPhase, JoinRows,
    PendingJoinDiagnostic, QueryError, QuerySource, SourceExecutionEnv, Value,
};
use crate::executor::semantic::SemanticKey;
use crate::types::DataType;

#[path = "bounded/source_retention.rs"]
mod source_retention;

use source_retention::{
    load_collection_rows, project_source_row, reserve_probe_key, SourceRowShape,
};

#[path = "bounded/side_selection.rs"]
mod side_selection;

#[path = "bounded/indexed.rs"]
mod indexed;

#[cfg(test)]
#[path = "bounded/tests.rs"]
mod tests;

use indexed::execute_indexed_bounded_inner_join;

#[path = "bounded/streaming.rs"]
mod streaming;

use streaming::{
    execute_dense_streaming_bounded_inner_join, execute_left_build_streaming_bounded_inner_join,
    load_streaming_right_rows, stream_left_rows_against_right, StreamingRightRows,
};

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

struct IndexedJoinSpec<'a> {
    left_collection: &'a str,
    right_collection: &'a str,
    on: &'a Expr,
    keys: &'a EquiJoinKeys,
    plan: &'a IndexedJoinPlan,
    output_budget: usize,
}

pub(super) fn try_execute_indexed_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &JoinExecutionSpec<'_>,
) -> Result<Option<JoinResult>, QueryError> {
    try_execute_indexed_bounded_inner_join_with_context(env, spec, &JoinRetentionContext::default())
}

fn try_execute_indexed_bounded_inner_join_with_context(
    env: &SourceExecutionEnv<'_>,
    spec: &JoinExecutionSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<Option<JoinResult>, QueryError> {
    if env
        .cassie
        .runtime
        .limits()
        .operator_switching_enabled
        .is_enabled()
    {
        return Ok(None);
    }
    let Some(output_budget) = spec.row_budget else {
        return Ok(None);
    };
    if !bounded_batch_fits_memory(env, output_budget) {
        return Ok(None);
    }
    if output_budget == 0 {
        return JoinResult::empty(env).map(Some);
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
        &IndexedJoinSpec {
            left_collection,
            right_collection,
            on: spec.on,
            keys: &keys,
            plan: &plan,
            output_budget,
        },
        retention,
    )
    .map(Some)
}

fn indexed_join_plan(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    right_collection: &str,
    keys: &EquiJoinKeys,
) -> Option<IndexedJoinPlan> {
    let left_field = join_field_for_collection(&keys.left, left_collection)?;
    let right_field = join_field_for_collection(&keys.right, right_collection)?;
    if has_mixed_numeric_index_encoding(
        env,
        left_collection,
        &left_field,
        right_collection,
        &right_field,
    ) {
        return None;
    }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum NumericIndexEncoding {
    Integer,
    Float,
}

fn has_mixed_numeric_index_encoding(
    env: &SourceExecutionEnv<'_>,
    left_collection: &str,
    left_field: &str,
    right_collection: &str,
    right_field: &str,
) -> bool {
    let left = collection_field_data_type(env, left_collection, left_field)
        .and_then(|data_type| numeric_index_encoding(&data_type));
    let right = collection_field_data_type(env, right_collection, right_field)
        .and_then(|data_type| numeric_index_encoding(&data_type));
    matches!((left, right), (Some(left), Some(right)) if left != right)
}

fn collection_field_data_type(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    field: &str,
) -> Option<DataType> {
    env.cassie
        .catalog
        .get_schema(collection)?
        .fields
        .into_iter()
        .find(|metadata| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(field, &metadata.name)
        })
        .map(|metadata| metadata.data_type)
}

const fn numeric_index_encoding(data_type: &DataType) -> Option<NumericIndexEncoding> {
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => {
            Some(NumericIndexEncoding::Integer)
        }
        DataType::Float => Some(NumericIndexEncoding::Float),
        _ => None,
    }
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
) -> Result<Option<JoinResult>, QueryError> {
    try_execute_streaming_bounded_inner_join_with_context(
        env,
        join,
        cte_context,
        &JoinRetentionContext::default(),
    )
}

fn try_execute_streaming_bounded_inner_join_with_context(
    env: &SourceExecutionEnv<'_>,
    join: &JoinExecutionSpec<'_>,
    _cte_context: &mut CteContext,
    retention: &JoinRetentionContext<'_>,
) -> Result<Option<JoinResult>, QueryError> {
    if env
        .cassie
        .runtime
        .limits()
        .operator_switching_enabled
        .is_enabled()
    {
        return Ok(None);
    }
    if join.row_budget == Some(0) {
        return JoinResult::empty(env).map(Some);
    }
    let Some(output_budget) = join.row_budget else {
        return Ok(None);
    };
    if !bounded_batch_fits_memory(env, output_budget) {
        return Ok(None);
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
    if should_preemptively_dense_stream(env, spec.left_collection, spec.right_collection)? {
        env.cassie
            .runtime
            .record_bounded_join_side_selection("dense_stream_preemptive_temp_budget");
        return execute_dense_streaming_bounded_inner_join(env, &spec, retention).map(Some);
    }
    let side_selection = side_selection::build_side_for_streaming(env, &spec)?;
    env.cassie
        .runtime
        .record_bounded_join_side_selection(side_selection.reason);
    if side_selection.build_left {
        return execute_left_build_streaming_bounded_inner_join(env, &spec, retention).map(Some);
    }

    execute_right_build_streaming_bounded_inner_join(env, &spec, retention).map(Some)
}

fn execute_right_build_streaming_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let limits = env.cassie.runtime.limits();
    let right_rows = match load_streaming_right_rows(env, spec, retention)? {
        StreamingRightRows::Rows(right_rows) => right_rows,
        StreamingRightRows::Dense(rows) => return Ok(rows),
    };
    if right_rows.is_empty() {
        return Ok(JoinResult::new(
            JoinRows::try_new(env.controls)?,
            PendingJoinDiagnostic::Vectorized {
                probe_rows: 0,
                build_rows: 0,
                matched_rows: 0,
                batch_size: limits.vectorized_join_batch_size.max(1),
                batches: 0,
            },
        ));
    }

    retention.enter(JoinRetentionPhase::BoundedBuild);
    let _build_memory =
        env.controls
            .reserve_query_memory(accounting::hash_build_bytes::<usize>(
                right_rows.as_slice(),
                &spec.keys.right,
            )?)?;
    retention.before(JoinRetentionPhase::BoundedBuild)?;
    let mut build = std::collections::HashMap::<SemanticKey, Vec<usize>>::new();
    build
        .try_reserve(accounting::hash_build_capacity(
            right_rows.as_slice(),
            &spec.keys.right,
        ))
        .map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to retain bounded join build: {error}"
            ))
        })?;
    for (index, right_row) in right_rows.as_slice().iter().enumerate() {
        check_timeout(env.controls)?;
        if let Some(key) = row_join_key(right_row, &spec.keys.right) {
            build.entry(key).or_default().push(index);
        }
    }

    let batch_size = limits.vectorized_join_batch_size.max(1);
    let progress =
        stream_left_rows_against_right(env, spec, &build, right_rows.as_slice(), retention)?;
    env.cassie.runtime.record_read_path_collection_scan(
        spec.left_collection,
        spec.left_scan_fields.len(),
        progress.scanned,
    );
    check_timeout(env.controls)?;
    Ok(JoinResult::new(
        progress.joined,
        PendingJoinDiagnostic::Vectorized {
            probe_rows: progress.probe_rows,
            build_rows: right_rows.len(),
            matched_rows: progress.matched_rows,
            batch_size,
            batches: progress.probe_rows.div_ceil(batch_size),
        },
    ))
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
                && index.normalized_fields().first().is_some_and(|candidate| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(field, candidate)
                })
                && crate::executor::execution::index_read::index_trailing_keys_not_null(
                    env.cassie, collection, index,
                )
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

fn has_session_changes(env: &SourceExecutionEnv<'_>, collection: &str) -> bool {
    env.session
        .is_some_and(|session| session.has_collection_changes(collection))
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
        env.controls.query_memory_budget_bytes <= estimated_batch_bytes
            && can_dense_stream(env, left_collection, right_collection)?,
    )
}

fn bounded_batch_fits_memory(env: &SourceExecutionEnv<'_>, output_budget: usize) -> bool {
    if output_budget < env.controls.max_result_rows.saturating_add(1) {
        return true;
    }
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    estimate_vectorized_join_bytes(batch_size, batch_size) <= env.controls.query_memory_budget_bytes
}

fn is_temp_budget_error(error: &QueryError) -> bool {
    matches!(
        error,
        QueryError::General(message)
            if message.starts_with("query memory budget exceeded:")
    ) || matches!(
        error,
        QueryError::Cassie(crate::app::CassieError::ResourceLimit(message))
            if message.starts_with("query memory budget exceeded:")
    )
}
