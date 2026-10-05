use std::mem::size_of;

use crate::executor::retained_memory::mul;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryMemoryReservation;

use super::{
    catalog, check_timeout, combine_rows, filter, project_source_row, reserve_probe_key, BatchRow,
    IndexedJoinSpec, JoinResult, JoinRetentionContext, JoinRetentionPhase, JoinRows, JoinSide,
    PendingJoinDiagnostic, QueryError, SourceExecutionEnv, SourceRowShape, Value,
};

struct IndexedProgress {
    joined: JoinRows,
    left_rows: usize,
    right_rows: usize,
    matched_rows: usize,
    index_scans: usize,
}

pub(super) fn execute_indexed_bounded_inner_join(
    env: &SourceExecutionEnv<'_>,
    spec: &IndexedJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let stream_collection = match spec.plan.indexed_side {
        JoinSide::Left => spec.right_collection,
        JoinSide::Right => spec.left_collection,
    };
    let stream_schema = env.cassie.catalog.get_schema(stream_collection);
    let batch_size = env
        .cassie
        .runtime
        .limits()
        .vectorized_join_batch_size
        .max(1);
    let mut progress = IndexedProgress {
        joined: JoinRows::try_new(env.controls)?,
        left_rows: 0,
        right_rows: 0,
        matched_rows: 0,
        index_scans: 0,
    };
    let streamed_rows = env.cassie.midge.scan_rows_until::<QueryError, _>(
        stream_collection,
        crate::midge::adapter::RowDecode::Full,
        env.controls,
        |document| {
            let stream_row = project_source_row(
                env,
                &document,
                SourceRowShape {
                    collection: stream_collection,
                    fields: &spec.plan.stream_scan_fields,
                    schema: stream_schema.as_ref(),
                },
                JoinRetentionPhase::BoundedSource,
                retention,
            )?;
            progress.probe_stream_row(env, spec, &stream_row, retention)
        },
    )?;
    env.cassie.runtime.record_read_path_collection_scan(
        stream_collection,
        spec.plan.stream_scan_fields.len(),
        streamed_rows,
    );
    check_timeout(env.controls)?;
    Ok(JoinResult::new(
        progress.joined,
        PendingJoinDiagnostic::Vectorized {
            probe_rows: progress.left_rows,
            build_rows: progress.right_rows,
            matched_rows: progress.matched_rows,
            batch_size,
            batches: progress.index_scans,
        },
    ))
}

impl IndexedProgress {
    fn probe_stream_row(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        spec: &IndexedJoinSpec<'_>,
        stream_row: &BatchRow,
        retention: &JoinRetentionContext<'_>,
    ) -> Result<bool, QueryError> {
        let (indexed_collection, stream_key) = match spec.plan.indexed_side {
            JoinSide::Left => {
                self.right_rows += 1;
                (spec.left_collection, &spec.keys.right)
            }
            JoinSide::Right => {
                self.left_rows += 1;
                (spec.right_collection, &spec.keys.left)
            }
        };
        let _key_memory = reserve_probe_key(env, stream_row, stream_key)?;
        let Some(key_value) = stream_row.get(stream_key).and_then(value_to_json) else {
            return Ok(true);
        };
        let remaining = spec.output_budget.saturating_sub(self.joined.len());
        if remaining == 0 {
            return Ok(false);
        }
        let indexed_rows = scan_indexed_join_rows(
            env,
            indexed_collection,
            &spec.plan.indexed_scan_fields,
            &spec.plan.index,
            key_value,
            remaining,
            retention,
        )?;
        self.index_scans += 1;
        for indexed_row in indexed_rows.as_slice() {
            let (left, right) = match spec.plan.indexed_side {
                JoinSide::Left => {
                    self.left_rows += 1;
                    (indexed_row, stream_row)
                }
                JoinSide::Right => {
                    self.right_rows += 1;
                    (stream_row, indexed_row)
                }
            };
            retention.enter(JoinRetentionPhase::BoundedOutput);
            let accepted = self.joined.try_push_combined(left, right, || {
                retention.before(JoinRetentionPhase::BoundedOutput)?;
                let combined = combine_rows(left, right);
                Ok(indexed_join_row_matches(env, &combined, spec.on)?.then_some(combined))
            })?;
            if accepted {
                self.matched_rows += 1;
                if self.joined.len() >= spec.output_budget {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn indexed_join_row_matches(
    env: &SourceExecutionEnv<'_>,
    row: &BatchRow,
    on: &super::Expr,
) -> Result<bool, QueryError> {
    filter::eval_scalar(
        row,
        on,
        env.params,
        None,
        env.user_functions,
        None,
        env.session,
    )?
    .is_true()
}

fn scan_indexed_join_rows(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    scan_fields: &[String],
    index: &catalog::IndexMeta,
    key_value: serde_json::Value,
    limit: usize,
    retention: &JoinRetentionContext<'_>,
) -> Result<AccountedVec<BatchRow>, QueryError> {
    let (hit_ids, _hit_memory) = scan_indexed_join_ids(env, collection, index, key_value, limit)?;
    env.cassie
        .runtime
        .record_read_path_index_seek(collection, hit_ids.len(), &index.name);
    let schema = env.cassie.catalog.get_schema(collection);
    let mut rows = AccountedVec::try_new(env.controls)?;
    for hit_id in hit_ids.as_slice() {
        check_timeout(env.controls)?;
        // Eligibility excludes staged changes on both sides. The controlled point reader
        // uses one read snapshot and retains its original decode guard through conversion.
        let Some(source) =
            env.cassie
                .midge
                .get_retrieval_document_controlled(collection, hit_id, env.controls)?
        else {
            continue;
        };
        let (document, _source_memory) = source.into_parts();
        let row = project_source_row(
            env,
            &document,
            SourceRowShape {
                collection,
                fields: scan_fields,
                schema: schema.as_ref(),
            },
            JoinRetentionPhase::BoundedIndexedPoint,
            retention,
        )?;
        rows.try_push_with(0, || row)?;
    }
    check_timeout(env.controls)?;
    Ok(rows)
}

fn scan_indexed_join_ids(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    index: &catalog::IndexMeta,
    key_value: serde_json::Value,
    limit: usize,
) -> Result<(AccountedVec<String>, AccountedVec<QueryMemoryReservation>), QueryError> {
    let field = index
        .fields
        .first()
        .or_else(|| (!index.field.is_empty()).then_some(&index.field))
        .ok_or_else(|| QueryError::General("scalar join index has no leading field".to_owned()))?;
    // At most two numeric probes exist. Their vector slots are admitted before allocation;
    // the caller's key reservation continues to own the transferred string key, if any.
    let _probe_memory = env
        .controls
        .reserve_query_memory(mul(2, size_of::<serde_json::Value>())?)?;
    let counterpart = crate::executor::execution::index_read::signed_zero_probe_counterpart(
        env.cassie, collection, field, &key_value,
    );
    let mut probe_values = Vec::with_capacity(2);
    probe_values.push(key_value);
    if let Some(counterpart) = counterpart {
        probe_values.push(counterpart);
    }
    let mut hit_ids = AccountedVec::<String>::try_new(env.controls)?;
    let mut hit_memory = AccountedVec::<QueryMemoryReservation>::try_new(env.controls)?;
    for value in probe_values {
        check_timeout(env.controls)?;
        let _request_memory = env
            .controls
            .reserve_query_memory(size_of::<serde_json::Value>())?;
        let hits = env.cassie.midge.scan_scalar_index_controlled(
            index,
            &crate::midge::adapter::ScalarIndexScanRequest {
                equality_prefix: vec![value],
                limit: Some(limit),
                ..Default::default()
            },
            env.controls,
        )?;
        let (hits, memory) = hits.into_parts();
        hit_memory.try_push_with(0, || memory)?;
        for hit in hits {
            // Preserve first-hit ordering and signed-zero duplicate suppression. The finite
            // bounded output limit makes this borrowed check avoid a second owning ID set.
            if hit_ids.len() < limit && !hit_ids.as_slice().iter().any(|id| id == &hit.id) {
                hit_ids.try_push_with(hit.id.capacity(), || hit.id)?;
            }
        }
    }
    check_timeout(env.controls)?;
    Ok((hit_ids, hit_memory))
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
