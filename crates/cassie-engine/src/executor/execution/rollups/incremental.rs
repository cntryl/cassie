use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use crate::catalog::{IndexKind, RollupMeta, RollupState};
use crate::midge::adapter::documents::RollupSourceChangeReport;
use crate::midge::adapter::time_series_indexes::{
    ControlledTimeSeriesDocumentScanOutcome, ControlledTimeSeriesIndexScanOutcome,
};
use crate::midge::adapter::{DocumentRef, DocumentWriteOp};
use crate::runtime::RollupGroupSnapshot;
use crate::sql::ast::{AggregateQuantifier, Expr};
use crate::types::{temporal::parse_timestamp_utc, DataType};

use super::{Cassie, QueryError, QueryExecutionControls};

const MAX_AFFECTED_GROUPS: usize = 256;
const MAX_AFFECTED_GROUP_BYTES: usize = 64 * 1024;
const DAILY_BUCKET_SECONDS: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    bucket_seconds: i64,
    partition: String,
}

#[derive(Default)]
struct AffectedGroups {
    count_delta: BTreeMap<GroupKey, i64>,
    retained_bytes: usize,
}

enum Attempt {
    Refreshed(Vec<RollupGroupSnapshot>),
    Fallback(&'static str),
}

enum SelectedGroupScan {
    Documents(
        Vec<DocumentRef>,
        Vec<cassie_core::request::RequestMemoryReservation>,
    ),
    Fallback(&'static str),
}

#[derive(Clone)]
enum AggregateKind {
    Count,
    Sum(String),
    Min(String),
    Max(String),
}

#[derive(Clone)]
struct AggregateSpec {
    alias: String,
    kind: AggregateKind,
}

struct AggregateValue {
    sum: Option<i64>,
    min: Option<i64>,
    max: Option<i64>,
}

pub(super) fn try_refresh_rollup(
    cassie: &Cassie,
    meta: &RollupMeta,
    report: &RollupSourceChangeReport,
    controls: &QueryExecutionControls,
) -> Result<bool, QueryError> {
    if report.rows.is_empty() && !report.exceeded_limit {
        cassie
            .runtime
            .record_incremental_rollup_fallback("change-report-unavailable");
        return Ok(false);
    }
    let attempt = try_refresh_rollup_inner(cassie, meta, report, controls)?;
    match attempt {
        Attempt::Refreshed(groups) => {
            cassie.runtime.record_incremental_rollup_groups(groups);
            Ok(true)
        }
        Attempt::Fallback(reason) => {
            cassie.runtime.record_incremental_rollup_fallback(reason);
            Ok(false)
        }
    }
}

fn try_refresh_rollup_inner(
    cassie: &Cassie,
    meta: &RollupMeta,
    report: &RollupSourceChangeReport,
    controls: &QueryExecutionControls,
) -> Result<Attempt, QueryError> {
    if report.exceeded_limit {
        return Ok(Attempt::Fallback("change-report-limit"));
    }
    if meta.version != RollupMeta::CURRENT_VERSION
        || meta.output_key_format != RollupMeta::CURRENT_OUTPUT_KEY_FORMAT
        || meta.state != RollupState::Ready
        || meta.refresh_cursor.lag_rows != 0
    {
        return Ok(Attempt::Fallback("rollup-not-current"));
    }
    let current_generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::from)?;
    if meta.refresh_cursor.source_generation.checked_add(1) != Some(current_generation) {
        return Ok(Attempt::Fallback("generation-gap"));
    }
    let Some(index) = matching_index(cassie, meta) else {
        return Ok(Attempt::Fallback("matching-index-unavailable"));
    };
    let Some(aggregates) = supported_aggregates(cassie, meta) else {
        return Ok(Attempt::Fallback("unsupported-rollup-definition"));
    };
    let Some(groups) = affected_groups(report, meta) else {
        return Ok(Attempt::Fallback("affected-group-limit"));
    };
    let _group_memory = controls.reserve_query_memory(groups.retained_bytes)?;
    if groups.count_delta.is_empty() {
        return Ok(Attempt::Fallback("no-affected-groups"));
    }
    let output_collection = meta.output_collection.clone();
    let source_collection = meta.source_collection.clone();
    let collections = [source_collection, output_collection];
    cassie.midge.with_collection_gates(&collections, || {
        refresh_selected_groups(
            cassie,
            meta,
            &index,
            &groups,
            &aggregates,
            current_generation,
            controls,
        )
    })
}

fn matching_index(cassie: &Cassie, meta: &RollupMeta) -> Option<crate::catalog::IndexMeta> {
    if !meta.bucket_width.trim().eq_ignore_ascii_case("1 day")
        || meta.origin.is_some()
        || meta.filter_expr.is_some()
        || meta.group_keys.len() != 1
    {
        return None;
    }
    let group_key = &meta.group_keys[0];
    let schema = cassie.midge.collection_schema(&meta.source_collection)?;
    let timestamp = schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(&meta.timestamp_field))?;
    let partition = schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(group_key))?;
    // NOT NULL is a catalog constraint; the stored field schema stays nullable.
    let not_null = cassie.catalog.not_null_fields(&meta.source_collection);
    let is_not_null = |name: &str| {
        not_null
            .iter()
            .any(|field| field.eq_ignore_ascii_case(name))
    };
    if !is_not_null(&timestamp.name)
        || !matches!(
            &timestamp.data_type,
            DataType::Timestamp
                | DataType::Extended(crate::types::extended::ExtendedType::TimestampTz { .. })
        )
        || !is_not_null(&partition.name)
        || !matches!(&partition.data_type, DataType::Text)
    {
        return None;
    }
    cassie
        .catalog
        .list_indexes(&meta.source_collection)
        .into_iter()
        .find(|index| {
            index.kind == IndexKind::TimeSeries
                && index.field.eq_ignore_ascii_case(&meta.timestamp_field)
                && index
                    .options
                    .get("bucket_width")
                    .is_some_and(|width| width.trim().eq_ignore_ascii_case("1 day"))
                && index
                    .options
                    .get("partition_by")
                    .is_some_and(|partition| partition.trim().eq_ignore_ascii_case(group_key))
        })
}

fn supported_aggregates(cassie: &Cassie, meta: &RollupMeta) -> Option<Vec<AggregateSpec>> {
    let schema = cassie.midge.collection_schema(&meta.source_collection)?;
    let mut aggregates = Vec::with_capacity(meta.aggregates.len());
    let mut has_count = false;
    for aggregate in &meta.aggregates {
        let Expr::Function(function) =
            crate::sql::parser::parse_expression(&aggregate.expression).ok()?
        else {
            return None;
        };
        if function.quantifier != AggregateQuantifier::Default
            || function.filter.is_some()
            || !function.order_by.is_empty()
        {
            return None;
        }
        let kind = if function.name.eq_ignore_ascii_case("count")
            && matches!(function.args.as_slice(), [Expr::Column(column)] if column == "*")
        {
            has_count = true;
            AggregateKind::Count
        } else {
            let [Expr::Column(column)] = function.args.as_slice() else {
                return None;
            };
            let field = schema
                .fields
                .iter()
                .find(|field| field.name.eq_ignore_ascii_case(column))?;
            if !matches!(
                &field.data_type,
                DataType::SmallInt | DataType::Int | DataType::BigInt
            ) {
                return None;
            }
            if function.name.eq_ignore_ascii_case("sum") {
                AggregateKind::Sum(field.name.clone())
            } else if function.name.eq_ignore_ascii_case("min") {
                AggregateKind::Min(field.name.clone())
            } else if function.name.eq_ignore_ascii_case("max") {
                AggregateKind::Max(field.name.clone())
            } else {
                return None;
            }
        };
        aggregates.push(AggregateSpec {
            alias: aggregate.alias.clone(),
            kind,
        });
    }
    has_count.then_some(aggregates)
}

fn affected_groups(report: &RollupSourceChangeReport, meta: &RollupMeta) -> Option<AffectedGroups> {
    let mut groups = AffectedGroups::default();
    let group_key = &meta.group_keys[0];
    for row in &report.rows {
        if let Some(before) = row.before.as_ref() {
            let group = group_for_payload(before, &meta.timestamp_field, group_key)?;
            add_group_delta(&mut groups, group, -1)?;
        }
        if let Some(after) = row.after.as_ref() {
            let group = group_for_payload(after, &meta.timestamp_field, group_key)?;
            add_group_delta(&mut groups, group, 1)?;
        }
    }
    (groups.count_delta.len() <= MAX_AFFECTED_GROUPS
        && groups.retained_bytes <= MAX_AFFECTED_GROUP_BYTES)
        .then_some(groups)
}

fn group_for_payload(
    payload: &serde_json::Map<String, serde_json::Value>,
    timestamp_field: &str,
    group_key: &str,
) -> Option<GroupKey> {
    let timestamp = payload.get(timestamp_field)?.as_str()?;
    let partition = payload.get(group_key)?.as_str()?;
    let timestamp = parse_timestamp_utc(timestamp).ok()?;
    let bucket_seconds =
        timestamp.unix_timestamp().div_euclid(DAILY_BUCKET_SECONDS) * DAILY_BUCKET_SECONDS;
    Some(GroupKey {
        bucket_seconds,
        partition: partition.to_owned(),
    })
}

fn add_group_delta(groups: &mut AffectedGroups, group: GroupKey, delta: i64) -> Option<()> {
    if !groups.count_delta.contains_key(&group) {
        let group_count = groups.count_delta.len().checked_add(1)?;
        if group_count > MAX_AFFECTED_GROUPS {
            return None;
        }
        let added_bytes = size_of::<GroupKey>()
            .checked_add(group.partition.capacity())?
            .checked_add(size_of::<i64>())?
            .checked_add(3 * size_of::<usize>())?;
        let retained_bytes = groups.retained_bytes.checked_add(added_bytes)?;
        if retained_bytes > MAX_AFFECTED_GROUP_BYTES {
            return None;
        }
        groups.retained_bytes = retained_bytes;
    }
    let count = groups.count_delta.entry(group).or_default();
    *count = count.checked_add(delta)?;
    Some(())
}

enum RecomputedGroup {
    Ready {
        operation: Option<DocumentWriteOp>,
        snapshot: RollupGroupSnapshot,
    },
    Fallback(&'static str),
}

fn refresh_selected_groups(
    cassie: &Cassie,
    meta: &RollupMeta,
    index: &crate::catalog::IndexMeta,
    groups: &AffectedGroups,
    aggregates: &[AggregateSpec],
    source_generation: u64,
    controls: &QueryExecutionControls,
) -> Result<Attempt, QueryError> {
    if cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::from)?
        != source_generation
    {
        return Ok(Attempt::Fallback("source-generation-changed"));
    }
    let operation_bytes = groups
        .count_delta
        .len()
        .checked_mul(size_of::<DocumentWriteOp>() + size_of::<RollupGroupSnapshot>())
        .ok_or_else(|| QueryError::General("incremental rollup admission overflow".to_string()))?;
    let mut operation_memory = controls.reserve_query_memory(operation_bytes)?;
    let capacity = groups.count_delta.len();
    let mut operations = Vec::new();
    operations
        .try_reserve_exact(capacity)
        .map_err(|error| QueryError::General(error.to_string()))?;
    if operations.capacity() > capacity {
        return Err(QueryError::General(
            "incremental rollup operation allocation exceeded its admission".to_string(),
        ));
    }
    let mut group_snapshots = Vec::new();
    group_snapshots
        .try_reserve_exact(capacity)
        .map_err(|error| QueryError::General(error.to_string()))?;
    if group_snapshots.capacity() > capacity {
        return Err(QueryError::General(
            "incremental rollup group allocation exceeded its admission".to_string(),
        ));
    }
    let context = RefreshContext {
        cassie,
        meta,
        index,
        aggregates,
        controls,
    };
    for (group, count_delta) in &groups.count_delta {
        match recompute_group(context, group, *count_delta, &mut operation_memory)? {
            RecomputedGroup::Ready {
                operation,
                snapshot,
            } => {
                if let Some(operation) = operation {
                    operations.push(operation);
                }
                group_snapshots.push(snapshot);
            }
            RecomputedGroup::Fallback(reason) => return Ok(Attempt::Fallback(reason)),
        }
        controls.check_interruption().map_err(QueryError::from)?;
    }
    publish_selected_groups(cassie, meta, operations, group_snapshots, source_generation)
}

/// The fixed inputs shared by every group recomputed in one refresh.
#[derive(Clone, Copy)]
struct RefreshContext<'a> {
    cassie: &'a Cassie,
    meta: &'a RollupMeta,
    index: &'a crate::catalog::IndexMeta,
    aggregates: &'a [AggregateSpec],
    controls: &'a QueryExecutionControls,
}

fn recompute_group(
    context: RefreshContext<'_>,
    group: &GroupKey,
    count_delta: i64,
    operation_memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<RecomputedGroup, QueryError> {
    let RefreshContext {
        cassie,
        meta,
        index,
        aggregates,
        controls,
    } = context;
    let Some(prior) = prior_group_count(cassie, meta, group, aggregates)? else {
        return Ok(RecomputedGroup::Fallback("rollup-count-missing"));
    };
    let Some(expected_count) = prior.checked_add(count_delta) else {
        return Ok(RecomputedGroup::Fallback("rollup-count-overflow"));
    };
    if expected_count < 0 {
        return Ok(RecomputedGroup::Fallback("rollup-count-underflow"));
    }
    let (documents, _scan_memory) = match scan_selected_group(cassie, index, meta, group, controls)?
    {
        SelectedGroupScan::Documents(documents, memory) => (documents, memory),
        SelectedGroupScan::Fallback(reason) => return Ok(RecomputedGroup::Fallback(reason)),
    };
    let observed_count = i64::try_from(documents.len())
        .map_err(|_| QueryError::General("rollup count exceeds int8".to_string()))?;
    if observed_count != expected_count {
        return Ok(RecomputedGroup::Fallback("rollup-count-mismatch"));
    }
    let operation = if documents.is_empty() {
        if prior > 0 {
            let id = super::stable_group_row_id(meta, group.bucket_seconds, &group.partition)?;
            operation_memory.try_grow(id.capacity())?;
            Some(DocumentWriteOp::Delete { id })
        } else {
            None
        }
    } else {
        let payload = aggregate_group_payload(meta, group, &documents, aggregates)?;
        let id = super::stable_rollup_row_id(meta, &payload)?;
        let payload_bytes = super::rollup_payload::json_value_heap_size(&payload)?;
        let retained_bytes = id.capacity().checked_add(payload_bytes).ok_or_else(|| {
            QueryError::General("incremental rollup admission overflow".to_string())
        })?;
        operation_memory.try_grow(retained_bytes)?;
        Some(DocumentWriteOp::Put { id, payload })
    };
    let snapshot = snapshot_group(group)?;
    let snapshot_bytes = snapshot
        .device
        .capacity()
        .checked_add(snapshot.bucket.capacity())
        .ok_or_else(|| QueryError::General("incremental rollup admission overflow".to_string()))?;
    operation_memory.try_grow(snapshot_bytes)?;
    Ok(RecomputedGroup::Ready {
        operation,
        snapshot,
    })
}

fn publish_selected_groups(
    cassie: &Cassie,
    meta: &RollupMeta,
    operations: Vec<DocumentWriteOp>,
    group_snapshots: Vec<RollupGroupSnapshot>,
    source_generation: u64,
) -> Result<Attempt, QueryError> {
    let generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::from)?;
    if generation != source_generation {
        return Ok(Attempt::Fallback("source-generation-changed"));
    }
    let mut updated = cassie
        .catalog
        .get_rollup(&meta.name)
        .ok_or_else(|| QueryError::General(format!("rollup '{}' disappeared", meta.name)))?;
    if updated.refresh_cursor.source_generation != meta.refresh_cursor.source_generation
        || updated.state != RollupState::Ready
    {
        return Ok(Attempt::Fallback("rollup-cursor-changed"));
    }
    updated.state = RollupState::Ready;
    updated.output_key_format = RollupMeta::CURRENT_OUTPUT_KEY_FORMAT;
    updated.refresh_cursor.last_refresh_ms = super::now_ms();
    updated.refresh_cursor.source_generation = source_generation;
    updated.refresh_cursor.source_epoch = cassie.runtime.data_epoch();
    updated.refresh_cursor.source_row_count = cassie
        .catalog
        .get_cardinality_stats(&meta.source_collection)
        .map_or(0, |stats| stats.row_count);
    updated.refresh_cursor.lag_rows = 0;
    let (database, key, value) = cassie
        .midge
        .rollup_data_write(&updated)
        .map_err(QueryError::from)?;
    let mut options =
        crate::midge::adapter::DocumentWriteBatchOptions::sync(cassie.midge.write_options_sync());
    options.data_database = Some(database);
    options.additional_data_writes.push((key, Some(value)));
    cassie
        .midge
        .apply_document_write_batch_with_options(&meta.output_collection, operations, &options)
        .map_err(QueryError::from)?;
    cassie.catalog.register_rollup(updated);
    cassie
        .refresh_cardinality_stats(&meta.output_collection)
        .map_err(QueryError::from)?;
    cassie.runtime.record_rollup_refresh(meta.name.clone());
    Ok(Attempt::Refreshed(group_snapshots))
}

fn prior_group_count(
    cassie: &Cassie,
    meta: &RollupMeta,
    group: &GroupKey,
    aggregates: &[AggregateSpec],
) -> Result<Option<i64>, QueryError> {
    let id = super::stable_group_row_id(meta, group.bucket_seconds, &group.partition)?;
    let Some(document) = cassie
        .midge
        .get_document(&meta.output_collection, &id)
        .map_err(QueryError::from)?
    else {
        return Ok(Some(0));
    };
    let Some(alias) = aggregates.iter().find_map(|aggregate| {
        matches!(&aggregate.kind, AggregateKind::Count).then_some(&aggregate.alias)
    }) else {
        return Ok(None);
    };
    Ok(document
        .payload
        .get(alias)
        .and_then(serde_json::Value::as_i64))
}

fn scan_selected_group(
    cassie: &Cassie,
    index: &crate::catalog::IndexMeta,
    meta: &RollupMeta,
    group: &GroupKey,
    controls: &QueryExecutionControls,
) -> Result<SelectedGroupScan, QueryError> {
    let upper = group
        .bucket_seconds
        .checked_add(DAILY_BUCKET_SECONDS)
        .ok_or_else(|| QueryError::General("rollup bucket upper bound overflow".to_string()))?;
    let (hits, scan_memory) = match cassie
        .midge
        .scan_time_series_index_controlled(
            index,
            Some(&group.partition),
            Some(group.bucket_seconds),
            Some(upper),
            &controls.storage_request(),
        )
        .map_err(QueryError::from)?
    {
        ControlledTimeSeriesIndexScanOutcome::Native(report) => (report.hits, report.memory),
        ControlledTimeSeriesIndexScanOutcome::Fallback(reason) => {
            return Ok(SelectedGroupScan::Fallback(reason));
        }
    };
    let mut fields = BTreeSet::from([meta.timestamp_field.clone(), meta.group_keys[0].clone()]);
    for aggregate in &meta.aggregates {
        let Expr::Function(function) = crate::sql::parser::parse_expression(&aggregate.expression)
            .map_err(|error| QueryError::General(error.to_string()))?
        else {
            return Ok(SelectedGroupScan::Fallback("unsupported-rollup-definition"));
        };
        if let Some(Expr::Column(field)) = function.args.first() {
            if field != "*" {
                fields.insert(field.clone());
            }
        }
    }
    let fields = fields.into_iter().collect::<Vec<_>>();
    match cassie
        .midge
        .scan_time_series_hit_documents_controlled(index, &hits, &fields, &controls.storage_request())
        .map_err(QueryError::from)?
    {
        ControlledTimeSeriesDocumentScanOutcome::Native(report) => {
            let mut memory = scan_memory;
            memory.push(report.memory);
            Ok(SelectedGroupScan::Documents(report.documents, memory))
        }
        ControlledTimeSeriesDocumentScanOutcome::Fallback(reason) => {
            Ok(SelectedGroupScan::Fallback(reason))
        }
    }
}

fn aggregate_group_payload(
    meta: &RollupMeta,
    group: &GroupKey,
    documents: &[DocumentRef],
    aggregates: &[AggregateSpec],
) -> Result<serde_json::Value, QueryError> {
    let bucket = time::OffsetDateTime::from_unix_timestamp(group.bucket_seconds)
        .map_err(|error| QueryError::General(error.to_string()))?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let mut payload = serde_json::Map::new();
    payload.insert(meta.bucket_expr.clone(), serde_json::Value::String(bucket));
    payload.insert(
        meta.group_keys[0].clone(),
        serde_json::Value::String(group.partition.clone()),
    );
    let count = i64::try_from(documents.len())
        .map_err(|_| QueryError::General("rollup count exceeds int8".to_string()))?;
    let mut values = aggregates
        .iter()
        .map(|aggregate| {
            (
                aggregate.alias.clone(),
                AggregateValue {
                    sum: None,
                    min: None,
                    max: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for aggregate in aggregates {
        match &aggregate.kind {
            AggregateKind::Count => {
                payload.insert(
                    aggregate.alias.clone(),
                    serde_json::Value::Number(count.into()),
                );
            }
            AggregateKind::Sum(field) | AggregateKind::Min(field) | AggregateKind::Max(field) => {
                let value = values
                    .get_mut(&aggregate.alias)
                    .expect("aggregate accumulator exists");
                for document in documents {
                    let Some(raw) = document.payload.get(field) else {
                        continue;
                    };
                    if raw.is_null() {
                        continue;
                    }
                    let Some(number) = raw.as_i64() else {
                        return Err(QueryError::General(format!(
                            "incremental rollup source field '{field}' is not an integer"
                        )));
                    };
                    match &aggregate.kind {
                        AggregateKind::Sum(_) => {
                            value.sum = Some(
                                value
                                    .sum
                                    .unwrap_or_default()
                                    .checked_add(number)
                                    .ok_or_else(|| {
                                        QueryError::General("rollup SUM overflow".to_string())
                                    })?,
                            );
                        }
                        AggregateKind::Min(_) => {
                            value.min =
                                Some(value.min.map_or(number, |current| current.min(number)));
                        }
                        AggregateKind::Max(_) => {
                            value.max =
                                Some(value.max.map_or(number, |current| current.max(number)));
                        }
                        AggregateKind::Count => unreachable!(),
                    }
                }
                let value = match &aggregate.kind {
                    AggregateKind::Sum(_) => value.sum,
                    AggregateKind::Min(_) => value.min,
                    AggregateKind::Max(_) => value.max,
                    AggregateKind::Count => unreachable!(),
                };
                payload.insert(
                    aggregate.alias.clone(),
                    value.map_or(serde_json::Value::Null, |number| {
                        serde_json::Value::Number(number.into())
                    }),
                );
            }
        }
    }
    Ok(serde_json::Value::Object(payload))
}

fn snapshot_group(group: &GroupKey) -> Result<RollupGroupSnapshot, QueryError> {
    let bucket = time::OffsetDateTime::from_unix_timestamp(group.bucket_seconds)
        .map_err(|error| QueryError::General(error.to_string()))?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(RollupGroupSnapshot {
        device: group.partition.clone(),
        bucket,
    })
}

#[cfg(test)]
#[path = "incremental/tests.rs"]
mod tests;
