use super::super::{Cassie, QueryError, QueryExecutionControls, QueryResult};
use crate::catalog::{RetentionPolicyMeta, RetentionScheduleState};
use crate::midge::adapter::{OrderedRowBound, OrderedRowScanRequest, RowDecode};

const BATCH_ROWS: usize = 256;
const MICROS_PER_DAY: i64 = 86_400_000_000;

pub(super) fn enforce_due(
    cassie: &Cassie,
    metadata: &RetentionPolicyMeta,
    due_at: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let tick = super::parse_timestamp(due_at)?;
    let tick_us = super::timestamp_micros(tick)?;
    let collections = cassie.referential_write_collections(&metadata.collection);
    let mut attempted = metadata.clone();
    let result = cassie.midge.with_collection_write_gates(&collections, || {
        let outcome = (|| {
            let mut current = current_policy(cassie, metadata);
            attempted.clone_from(&current);
            if current.schedule_state == RetentionScheduleState::Unscheduled {
                return Err(QueryError::General(format!(
                    "retention policy '{}' has no schedule",
                    current.name
                )));
            }
            if current.schedule_state == RetentionScheduleState::Suspended {
                return Ok(0);
            }
            if current.pending_cutoff_us.is_none() {
                let Some(next_due_at_us) = current.next_due_at_us else {
                    return Err(QueryError::General(format!(
                        "retention policy '{}' is missing its next due time",
                        current.name
                    )));
                };
                if tick < super::timestamp_from_micros(next_due_at_us)? {
                    return Ok(0);
                }
                current.pending_cutoff_us = Some(tick_us);
                current.pending_last_row_id = None;
                current.pending_scanned_rows = 0;
                current.pending_deleted_rows = 0;
                current.pending_skipped_rows = 0;
                attempted.clone_from(&current);
            }
            stage_and_publish_batch(cassie, current, controls)
        })();
        record_failed_attempt(cassie, &attempted, &outcome);
        outcome
    });
    result.and_then(|deleted| command_count(deleted, controls))
}

pub(super) fn replay_pending(
    cassie: &Cassie,
    metadata: &RetentionPolicyMeta,
    at: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    // Parse the requested AT value for SQL validation, but keep the durable
    // original DUE AT instant while a scheduled window is unfinished.
    let _requested_at = super::parse_timestamp(at)?;
    let collections = cassie.referential_write_collections(&metadata.collection);
    let mut attempted = metadata.clone();
    let result = cassie.midge.with_collection_write_gates(&collections, || {
        let outcome = (|| {
            let current = current_policy(cassie, metadata);
            attempted.clone_from(&current);
            if current.pending_cutoff_us.is_none() {
                return Ok(0);
            }
            stage_and_publish_batch(cassie, current, controls)
        })();
        record_failed_attempt(cassie, &attempted, &outcome);
        outcome
    });
    result.and_then(|deleted| command_count(deleted, controls))
}

fn current_policy(cassie: &Cassie, fallback: &RetentionPolicyMeta) -> RetentionPolicyMeta {
    cassie
        .catalog
        .get_retention_policy(&fallback.name)
        .unwrap_or_else(|| fallback.clone())
}

/// Records a failed attempt while the caller still holds the collection write
/// gates, so an overlapping tick cannot commit progress between the durable
/// reread in [`persist_failure`] and its metadata write.
fn record_failed_attempt(
    cassie: &Cassie,
    attempted: &RetentionPolicyMeta,
    outcome: &Result<u64, QueryError>,
) {
    if let Err(error) = outcome {
        persist_failure(cassie, attempted.clone(), error);
    }
}

struct StagedRetentionRows {
    batch: crate::app::StatementMutationBatch,
    scanned: u64,
    deleted: u64,
    skipped: u64,
    last_row_id: Option<String>,
}

fn stage_and_publish_batch(
    cassie: &Cassie,
    mut metadata: RetentionPolicyMeta,
    controls: &QueryExecutionControls,
) -> Result<u64, QueryError> {
    if cassie
        .midge
        .collection_uses_column_store(&metadata.collection)
        .map_err(QueryError::from)?
    {
        return Err(QueryError::General(
            "scheduled retention requires row storage".to_owned(),
        ));
    }
    let cutoff_us = metadata.pending_cutoff_us.ok_or_else(|| {
        QueryError::General("scheduled retention has no pending cutoff".to_owned())
    })?;
    let enforce_at = super::timestamp_from_micros(cutoff_us)?;
    let delete_before = enforce_at - super::parse_duration(&metadata.retention_duration)?;
    let staged = stage_retention_rows(cassie, &metadata, delete_before, controls)?;
    let batch_rows = u64::try_from(BATCH_ROWS).map_err(|_| {
        QueryError::General("scheduled retention batch size exceeds u64".to_owned())
    })?;
    let has_more = staged.scanned == batch_rows;

    metadata.pending_scanned_rows = metadata
        .pending_scanned_rows
        .checked_add(staged.scanned)
        .ok_or_else(|| QueryError::General("scheduled retention counter overflow".to_owned()))?;
    metadata.pending_deleted_rows = metadata
        .pending_deleted_rows
        .checked_add(staged.deleted)
        .ok_or_else(|| QueryError::General("scheduled retention counter overflow".to_owned()))?;
    metadata.pending_skipped_rows = metadata
        .pending_skipped_rows
        .checked_add(staged.skipped)
        .ok_or_else(|| QueryError::General("scheduled retention counter overflow".to_owned()))?;
    metadata.pending_last_row_id = if has_more { staged.last_row_id } else { None };
    metadata.state = crate::catalog::RetentionPolicyState::Ready;
    metadata.last_error = None;
    if !has_more {
        complete_cutoff(&mut metadata, cutoff_us, enforce_at)?;
    }
    controls.check_interruption().map_err(QueryError::from)?;
    let policy_name = metadata.name.clone();
    let (database, key, value) = cassie
        .midge
        .retention_policy_data_write(&metadata)
        .map_err(QueryError::from)?;
    cassie
        .publish_statement_batch_with_data_writes(
            staged.batch,
            controls,
            database,
            vec![(key, Some(value))],
        )
        .map_err(QueryError::from)?;
    cassie.catalog.register_retention_policy(metadata);
    cassie
        .runtime
        .record_retention_enforcement(policy_name, staged.deleted, staged.skipped);
    Ok(staged.deleted)
}

fn complete_cutoff(
    metadata: &mut RetentionPolicyMeta,
    cutoff_us: i64,
    enforce_at: time::OffsetDateTime,
) -> Result<(), QueryError> {
    metadata.last_successful_cutoff_us = Some(cutoff_us);
    metadata.last_deleted_rows = metadata.pending_deleted_rows;
    metadata.last_skipped_rows = metadata.pending_skipped_rows;
    metadata.last_enforced_ms = Some(super::timestamp_millis(enforce_at));
    metadata.pending_cutoff_us = None;
    metadata.pending_last_row_id = None;
    metadata.pending_scanned_rows = 0;
    metadata.pending_deleted_rows = 0;
    metadata.pending_skipped_rows = 0;
    let next_due = metadata
        .next_due_at_us
        .unwrap_or(cutoff_us)
        .checked_add(MICROS_PER_DAY)
        .ok_or_else(|| QueryError::General("next retention due time overflow".to_owned()))?;
    metadata.next_due_at_us = Some(next_due);
    Ok(())
}

fn stage_retention_rows(
    cassie: &Cassie,
    metadata: &RetentionPolicyMeta,
    delete_before: time::OffsetDateTime,
    controls: &QueryExecutionControls,
) -> Result<StagedRetentionRows, QueryError> {
    let batch = cassie.new_statement_batch(None).map_err(QueryError::from)?;
    let start = metadata
        .pending_last_row_id
        .as_ref()
        .map(|id| OrderedRowBound {
            id: id.clone(),
            inclusive: false,
        });
    let mut cursor = cassie
        .midge
        .open_controlled_ordered_row_cursor(
            OrderedRowScanRequest {
                collection: &metadata.collection,
                decode: RowDecode::Full,
                start_bound: start.as_ref(),
                end_bound: None,
                reverse: false,
                limit: Some(BATCH_ROWS),
            },
            controls,
        )
        .map_err(QueryError::from)?;
    let mut staged = StagedRetentionRows {
        batch,
        scanned: 0,
        deleted: 0,
        skipped: 0,
        last_row_id: metadata.pending_last_row_id.clone(),
    };
    while let Some(accounted) = cursor
        .next_accounted_document(&cassie.midge, controls)
        .map_err(QueryError::from)?
    {
        let (document, _memory) = accounted.into_parts();
        staged.scanned = increment(staged.scanned)?;
        staged.last_row_id = Some(document.id.clone());
        account_retention_row(
            cassie,
            &mut staged,
            metadata,
            &document,
            delete_before,
            controls,
        )?;
    }
    Ok(staged)
}

fn account_retention_row(
    cassie: &Cassie,
    staged: &mut StagedRetentionRows,
    metadata: &RetentionPolicyMeta,
    document: &crate::midge::adapter::DocumentRef,
    delete_before: time::OffsetDateTime,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let Some(value) = document.payload.get(&metadata.timestamp_field) else {
        staged.skipped = increment(staged.skipped)?;
        return Ok(());
    };
    let Some(timestamp) = value
        .as_str()
        .and_then(|raw| super::parse_timestamp(raw).ok())
    else {
        staged.skipped = increment(staged.skipped)?;
        return Ok(());
    };
    if timestamp >= delete_before
        || staged
            .batch
            .session()
            .document_change(&metadata.collection, &document.id)
            .is_some_and(|change| matches!(change, crate::app::TransactionRowChange::Delete))
    {
        return Ok(());
    }
    match super::super::dml_referential_actions::delete_existing_row(
        cassie,
        Some(staged.batch.session()),
        &metadata.collection,
        &document.id,
        &document.payload,
        controls,
    )? {
        super::super::dml_referential_actions::DeleteOutcome::Deleted(true) => {
            staged.deleted = increment(staged.deleted)?;
        }
        super::super::dml_referential_actions::DeleteOutcome::Restricted(_) => {
            staged.skipped = increment(staged.skipped)?;
        }
        super::super::dml_referential_actions::DeleteOutcome::Deleted(false) => {}
    }
    Ok(())
}

fn increment(value: u64) -> Result<u64, QueryError> {
    value
        .checked_add(1)
        .ok_or_else(|| QueryError::General("scheduled retention count overflow".to_owned()))
}

fn command_count(
    deleted: u64,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut output = QueryResult::try_builder(controls)?;
    super::super::command_output::set_counted(&mut output, "ENFORCE RETENTION", deleted)?;
    Ok(output.finish()?)
}

fn persist_failure(cassie: &Cassie, mut metadata: RetentionPolicyMeta, error: &QueryError) {
    let persisted = cassie
        .midge
        .list_retention_policies()
        .ok()
        .and_then(|policies| {
            policies
                .into_iter()
                .find(|policy| policy.name == metadata.name)
        });
    if let Some(current) = persisted.filter(|current| {
        current.last_successful_cutoff_us != metadata.last_successful_cutoff_us
            || current.pending_scanned_rows > metadata.pending_scanned_rows
            || current.pending_deleted_rows > metadata.pending_deleted_rows
            || current.pending_skipped_rows > metadata.pending_skipped_rows
    }) {
        metadata = current;
    }
    let message = error.to_string();
    metadata.state = crate::catalog::RetentionPolicyState::Error;
    metadata.last_error = Some(message.clone());
    metadata.last_failure = Some(message);
    let _ = cassie.midge.put_retention_policy(&metadata);
    cassie.catalog.register_retention_policy(metadata.clone());
    cassie
        .runtime
        .record_retention_error(metadata.name, error.to_string());
}
