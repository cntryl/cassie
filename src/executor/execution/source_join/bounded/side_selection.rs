use super::{
    can_dense_stream, catalog, check_timeout, estimate_vectorized_join_bytes, hydrated_row_count,
    join_field_for_collection, qualify_row, row_join_key, scan, QueryError, SourceExecutionEnv,
    StreamingJoinSpec, ROW_COUNT_BUILD_SIDE_RATIO,
};

const FANOUT_BUILD_SIDE_COST_RATIO: u64 = 2;
const ROW_COUNT_SAMPLE_BUILD_SIDE_RATIO: u64 = 2;
const ROW_COUNT_SAMPLE_OUTPUT_RATIO: u64 = 2;
const MAX_JOIN_KEY_SAMPLE_ROWS: usize = 128;
const SAMPLE_MATCH_RATIO: usize = 2;

struct JoinFieldCardinality {
    rows: u64,
    non_null_rows: u64,
    distinct_values: u64,
}

pub(super) fn should_build_left_for_streaming(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
) -> Result<bool, QueryError> {
    let Some(left_rows) = hydrated_row_count(env, spec.left_collection) else {
        return Ok(false);
    };
    let Some(right_rows) = hydrated_row_count(env, spec.right_collection) else {
        return Ok(false);
    };
    let Ok(left_rows_usize) = usize::try_from(left_rows) else {
        return Ok(false);
    };
    if estimate_vectorized_join_bytes(left_rows_usize, 0) > env.controls.temp_spill_budget_bytes {
        return Ok(false);
    }
    if !can_dense_stream(env, spec.left_collection, spec.right_collection)? {
        return Ok(false);
    }

    if right_rows >= left_rows.saturating_mul(ROW_COUNT_BUILD_SIDE_RATIO)
        || should_build_left_from_fanout_stats(env, spec)
    {
        return Ok(true);
    }

    should_build_left_from_row_count_sample(env, spec, left_rows, right_rows)
}

fn should_build_left_from_fanout_stats(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
) -> bool {
    let Some(left_field) = join_field_for_collection(&spec.keys.left, spec.left_collection) else {
        return false;
    };
    let Some(right_field) = join_field_for_collection(&spec.keys.right, spec.right_collection)
    else {
        return false;
    };
    let Some(left) = hydrated_join_field_cardinality(env, spec.left_collection, &left_field) else {
        return false;
    };
    let Some(right) = hydrated_join_field_cardinality(env, spec.right_collection, &right_field)
    else {
        return false;
    };
    if left.rows >= right.rows {
        return false;
    }

    let left_build_cost = estimated_bounded_join_cost(spec.output_budget, &left, &right);
    let right_build_cost = estimated_bounded_join_cost(spec.output_budget, &right, &left);
    left_build_cost.saturating_mul(FANOUT_BUILD_SIDE_COST_RATIO) <= right_build_cost
}

fn should_build_left_from_row_count_sample(
    env: &SourceExecutionEnv<'_>,
    spec: &StreamingJoinSpec<'_>,
    left_rows: u64,
    right_rows: u64,
) -> Result<bool, QueryError> {
    if right_rows < left_rows.saturating_mul(ROW_COUNT_SAMPLE_BUILD_SIDE_RATIO)
        || !output_budget_can_use_left_build(spec.output_budget, left_rows)
    {
        return Ok(false);
    }

    let sample_limit = join_key_sample_limit(env, spec.output_budget);
    let left_keys = sample_join_keys(
        env,
        spec.left_collection,
        &spec.keys.left,
        &spec.left_scan_fields,
        sample_limit,
    )?;
    let right_keys = sample_join_keys(
        env,
        spec.right_collection,
        &spec.keys.right,
        &spec.right_scan_fields,
        sample_limit,
    )?;
    Ok(samples_support_left_build(&left_keys, &right_keys))
}

fn output_budget_can_use_left_build(output_budget: usize, left_rows: u64) -> bool {
    u64::try_from(output_budget)
        .unwrap_or(u64::MAX)
        .saturating_mul(ROW_COUNT_SAMPLE_OUTPUT_RATIO)
        >= left_rows
}

fn join_key_sample_limit(env: &SourceExecutionEnv<'_>, output_budget: usize) -> usize {
    env.cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .clamp(1, MAX_JOIN_KEY_SAMPLE_ROWS)
        .min(output_budget.max(1))
}

fn sample_join_keys(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    key: &str,
    scan_fields: &[String],
    limit: usize,
) -> Result<Vec<String>, QueryError> {
    let schema = env.cassie.catalog.get_schema(collection);
    let mut keys = Vec::with_capacity(limit);
    let scanned = env.cassie.midge.scan_rows_until::<QueryError, _>(
        collection,
        crate::midge::adapter::RowDecode::Full,
        |document| {
            check_timeout(env.controls)?;
            let row = qualify_row(
                scan::projected_document_to_row(document, scan_fields, schema.as_ref()),
                collection,
            );
            keys.push(row_join_key(&row, key));
            Ok(keys.len() < limit)
        },
    )?;
    env.cassie
        .runtime
        .record_read_path_collection_scan(collection, scan_fields.len(), scanned);
    Ok(keys)
}

fn samples_support_left_build(left_keys: &[String], right_keys: &[String]) -> bool {
    if left_keys.is_empty() || right_keys.is_empty() {
        return false;
    }

    let left_distinct = left_keys
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let matching_right_keys = right_keys
        .iter()
        .filter(|key| left_distinct.contains(key.as_str()))
        .count();
    matching_right_keys.saturating_mul(SAMPLE_MATCH_RATIO) >= right_keys.len()
}

fn hydrated_join_field_cardinality(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    field: &str,
) -> Option<JoinFieldCardinality> {
    let stats = env
        .cassie
        .catalog
        .get_cardinality_stats(collection)
        .filter(|stats| stats.hydrated)?;
    let field_stats = join_field_stats(&stats, field)?;
    if field_stats.stale_reason.is_some()
        || field_stats.confidence < 100
        || field_stats.sample_count < stats.row_count
        || field_stats.non_null_count != stats.row_count
        || field_stats.distinct_count == 0
    {
        return None;
    }

    Some(JoinFieldCardinality {
        rows: stats.row_count,
        non_null_rows: field_stats.non_null_count,
        distinct_values: field_stats.distinct_count,
    })
}

fn join_field_stats<'a>(
    stats: &'a catalog::CollectionCardinalityStats,
    field: &str,
) -> Option<&'a catalog::FieldCardinalityStats> {
    stats.field_stats(field).or_else(|| {
        stats
            .fields
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(field))
            .map(|(_, stats)| stats)
    })
}

fn estimated_bounded_join_cost(
    output_budget: usize,
    build: &JoinFieldCardinality,
    stream: &JoinFieldCardinality,
) -> u64 {
    let output_budget = u64::try_from(output_budget).unwrap_or(u64::MAX);
    let estimated_probe_rows = output_budget
        .saturating_mul(build.distinct_values)
        .div_ceil(build.non_null_rows.max(1))
        .min(stream.rows);
    build.rows.saturating_add(estimated_probe_rows)
}
