use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::midge::adapter::{
    DocumentWriteOp, PublicationControlScope, documents::ReplayBatchCommit,
};

use super::{Cassie, CassieError};

#[cfg(test)]
thread_local! {
    static POSTCOMMIT_READ_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(super) fn inject_replay_postcommit_metadata_read_failure() {
    POSTCOMMIT_READ_FAILURE.with(|flag| flag.set(true));
}

type ReplayPrepareBarriers = (
    String,
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);

static REPLAY_PREPARE_BARRIERS: std::sync::OnceLock<
    std::sync::Mutex<Option<ReplayPrepareBarriers>>,
> = std::sync::OnceLock::new();
static PROJECTION_CONCURRENCY_TEST_GUARD: std::sync::OnceLock<parking_lot::Mutex<()>> =
    std::sync::OnceLock::new();

#[doc(hidden)]
pub fn projection_concurrency_test_guard() -> parking_lot::MutexGuard<'static, ()> {
    PROJECTION_CONCURRENCY_TEST_GUARD
        .get_or_init(|| parking_lot::Mutex::new(()))
        .lock()
}

#[doc(hidden)]
pub fn set_projection_replay_prepare_barriers(
    batch_id: Option<String>,
    ready: Option<std::sync::Arc<std::sync::Barrier>>,
    resume: Option<std::sync::Arc<std::sync::Barrier>>,
) {
    *REPLAY_PREPARE_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("projection replay prepare barrier mutex") = batch_id
        .zip(ready)
        .zip(resume)
        .map(|((batch_id, ready), resume)| (batch_id, ready, resume));
}

fn pause_projection_replay_after_prepare(batch_id: &str) {
    let barriers = {
        let mut installed = REPLAY_PREPARE_BARRIERS
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .expect("projection replay prepare barrier mutex");
        if installed
            .as_ref()
            .is_some_and(|(target, _, _)| target == batch_id)
        {
            installed.take()
        } else {
            None
        }
    };
    if let Some((_, ready, resume)) = barriers {
        ready.wait();
        resume.wait();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionReplayEvent {
    pub event_id: String,
    pub checkpoint: String,
    pub position: Option<u64>,
    pub document_id: String,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionReplayBatch {
    pub projection: String,
    pub source_identity: String,
    pub batch_id: String,
    pub lag: u64,
    pub events: Vec<ProjectionReplayEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionReplayReport {
    pub applied_event_count: u64,
    pub skipped_duplicate_count: u64,
    pub freshness: crate::catalog::ProjectionFreshness,
    pub source_checkpoint: Option<String>,
    pub last_applied_event_id: Option<String>,
}

struct PreparedReplay {
    applied: u64,
    skipped: u64,
    duplicate_checks: u64,
    write_ops: Vec<DocumentWriteOp>,
    replay_event_ids: Vec<String>,
    observed_events: Vec<(String, bool)>,
    source_checkpoint: Option<String>,
    source_position: Option<u64>,
    last_applied_event_id: Option<String>,
}

impl Cassie {
    /// Replay a reconstructible projection with buffered local WAL or the
    /// configured cloud durability policy. A successful result need not survive
    /// loss of local storage; callers must reconcile the recovered checkpoint
    /// with retained source events. Use [`Self::replay_projection_batch_strict`]
    /// when the acknowledgement must use local sync or cloud-strict durability.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn replay_projection_batch(
        &self,
        batch: ProjectionReplayBatch,
    ) -> Result<ProjectionReplayReport, CassieError> {
        self.replay_projection_batch_with_options(batch, self.midge.write_options_replayable())
    }

    /// Replay with local synchronous or cloud-strict durability.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn replay_projection_batch_strict(
        &self,
        batch: ProjectionReplayBatch,
    ) -> Result<ProjectionReplayReport, CassieError> {
        self.replay_projection_batch_with_options(batch, self.midge.write_options_strict_replay())
    }

    /// Replay like [`Self::replay_projection_batch`], observing `cancellation`
    /// together with any ambient request cancellation or deadline.
    ///
    /// Cancellation observed before native commit submission returns
    /// [`CassieError::QueryCanceled`] and leaves rows, derived indexes, the event
    /// ledger and the checkpoint unchanged; it does not mark the projection
    /// failed, so the same batch can be retried. Once submission begins the
    /// actual outcome is reported: a confirmed commit returns success despite a
    /// later cancellation, and an unknown outcome stays fenced for
    /// reconciliation and is never reported as a cancellation.
    ///
    /// # Errors
    ///
    /// Returns an error when canceled before submission, or when validation,
    /// storage, or execution fails.
    pub fn replay_projection_batch_cancellable(
        &self,
        batch: ProjectionReplayBatch,
        cancellation: &super::QueryCancellationHandle,
    ) -> Result<ProjectionReplayReport, CassieError> {
        let _scope = PublicationControlScope::enter_for_replay(cancellation);
        self.replay_projection_batch_controlled(batch, self.midge.write_options_replayable())
    }

    /// Cancellable replay with local synchronous or cloud-strict durability.
    /// See [`Self::replay_projection_batch_cancellable`] for the cancellation
    /// contract.
    ///
    /// # Errors
    ///
    /// Returns an error when canceled before submission, or when validation,
    /// storage, or execution fails.
    pub fn replay_projection_batch_strict_cancellable(
        &self,
        batch: ProjectionReplayBatch,
        cancellation: &super::QueryCancellationHandle,
    ) -> Result<ProjectionReplayReport, CassieError> {
        let _scope = PublicationControlScope::enter_for_replay(cancellation);
        self.replay_projection_batch_controlled(batch, self.midge.write_options_strict_replay())
    }

    fn replay_projection_batch_controlled(
        &self,
        batch: ProjectionReplayBatch,
        commit: cntryl_midge::WriteOptions,
    ) -> Result<ProjectionReplayReport, CassieError> {
        Self::validate_replay_batch(&batch)?;
        let gated_projection = batch.projection.clone();
        self.midge.with_collection_write_gates_interruptible(
            std::slice::from_ref(&gated_projection),
            PublicationControlScope::check_current,
            || self.replay_projection_batch_gated(batch, commit),
        )
    }

    fn replay_projection_batch_with_options(
        &self,
        batch: ProjectionReplayBatch,
        commit: cntryl_midge::WriteOptions,
    ) -> Result<ProjectionReplayReport, CassieError> {
        Self::validate_replay_batch(&batch)?;
        let gated_projection = batch.projection.clone();
        self.midge
            .with_collection_write_gates(std::slice::from_ref(&gated_projection), || {
                self.replay_projection_batch_gated(batch, commit)
            })
    }

    fn replay_projection_batch_gated(
        &self,
        batch: ProjectionReplayBatch,
        commit: cntryl_midge::WriteOptions,
    ) -> Result<ProjectionReplayReport, CassieError> {
        let stored_metadata = self.midge.projection_metadata(&batch.projection)?;
        let metadata = stored_metadata
            .clone()
            .or_else(|| self.catalog.get_projection_metadata(&batch.projection))
            .ok_or_else(|| CassieError::CollectionNotFound(batch.projection.clone()))?;
        let metadata =
            self.ensure_replay_source_identity(metadata, stored_metadata.as_ref(), &batch)?;
        let prepared =
            self.prepare_projection_replay(&metadata, stored_metadata.as_ref(), &batch)?;
        pause_projection_replay_after_prepare(&batch.batch_id);
        PublicationControlScope::check_current()?;
        self.finalize_projection_replay(metadata, batch, prepared, stored_metadata, commit)
    }

    fn validate_replay_batch(batch: &ProjectionReplayBatch) -> Result<(), CassieError> {
        if batch.projection.trim().is_empty() {
            return Err(CassieError::Execution(
                "projection replay requires a projection".to_string(),
            ));
        }
        if batch.source_identity.trim().is_empty() {
            return Err(CassieError::Execution(
                "projection replay requires a source identity".to_string(),
            ));
        }
        Ok(())
    }

    fn ensure_replay_source_identity(
        &self,
        metadata: crate::catalog::ProjectionMeta,
        expected: Option<&crate::catalog::ProjectionMeta>,
        batch: &ProjectionReplayBatch,
    ) -> Result<crate::catalog::ProjectionMeta, CassieError> {
        if let Some(existing) = metadata.source_identity.clone() {
            if existing != batch.source_identity {
                return self.fail_replay_metadata(
                    metadata,
                    expected,
                    batch,
                    format!(
                        "projection '{}' is bound to source '{existing}', not '{}'",
                        batch.projection, batch.source_identity
                    ),
                );
            }
        }
        Ok(metadata)
    }

    fn prepare_projection_replay(
        &self,
        metadata: &crate::catalog::ProjectionMeta,
        expected: Option<&crate::catalog::ProjectionMeta>,
        batch: &ProjectionReplayBatch,
    ) -> Result<PreparedReplay, CassieError> {
        let mut applied = 0u64;
        let mut skipped = 0u64;
        let mut duplicate_checks = 0u64;
        let mut write_ops = Vec::new();
        let mut replay_event_ids = Vec::new();
        let mut batch_event_ids = BTreeSet::new();

        let mut position_cursor = metadata.source_position;
        let mut source_checkpoint = metadata.source_checkpoint.clone();
        let mut source_position = metadata.source_position;
        let mut last_applied_event_id = metadata.last_applied_event_id.clone();

        for event in &batch.events {
            if event.event_id.trim().is_empty() {
                return self.fail_replay_metadata(
                    metadata.clone(),
                    expected,
                    batch,
                    "projection replay event id cannot be empty".to_string(),
                );
            }
        }

        let event_ids = batch
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>();
        let event_seen = self.midge.projection_events_seen(
            &batch.projection,
            &batch.source_identity,
            &event_ids,
        )?;
        let observed_events = batch
            .events
            .iter()
            .zip(&event_seen)
            .map(|(event, seen)| (event.event_id.clone(), *seen))
            .collect();

        for (event, already_seen) in batch.events.iter().zip(event_seen) {
            PublicationControlScope::check_current()?;
            duplicate_checks = duplicate_checks.saturating_add(1);
            if already_seen {
                skipped = skipped.saturating_add(1);
                continue;
            }
            if !batch_event_ids.insert(event.event_id.clone()) {
                return self.fail_replay_metadata(
                    metadata.clone(),
                    expected,
                    batch,
                    format!(
                        "duplicate projection replay event '{}' in batch '{}'",
                        event.event_id, batch.batch_id
                    ),
                );
            }
            if let (Some(previous), Some(next)) = (position_cursor, event.position) {
                if next <= previous {
                    return self.fail_replay_metadata(
                        metadata.clone(),
                        expected,
                        batch,
                        format!(
                            "out-of-order projection replay event '{}' at position {next} after {previous}",
                            event.event_id
                        ),
                    );
                }
            }

            if let Some(payload) = event.payload.clone() {
                write_ops.push(DocumentWriteOp::Put {
                    id: event.document_id.clone(),
                    payload,
                });
            } else {
                write_ops.push(DocumentWriteOp::Delete {
                    id: event.document_id.clone(),
                });
            }
            replay_event_ids.push(event.event_id.clone());

            position_cursor = event.position;
            applied = applied.saturating_add(1);
            source_checkpoint = Some(event.checkpoint.clone());
            source_position = event.position;
            last_applied_event_id = Some(event.event_id.clone());
        }

        Ok(PreparedReplay {
            applied,
            skipped,
            duplicate_checks,
            write_ops,
            replay_event_ids,
            observed_events,
            source_checkpoint,
            source_position,
            last_applied_event_id,
        })
    }

    fn finalize_projection_replay(
        &self,
        mut metadata: crate::catalog::ProjectionMeta,
        batch: ProjectionReplayBatch,
        mut prepared: PreparedReplay,
        stored_metadata: Option<crate::catalog::ProjectionMeta>,
        commit: cntryl_midge::WriteOptions,
    ) -> Result<ProjectionReplayReport, CassieError> {
        let previous_metadata = metadata.clone();
        if metadata.source_identity.is_none() {
            metadata.source_identity = Some(batch.source_identity.clone());
        }
        metadata.replay_batch_id = Some(batch.batch_id.clone());
        metadata.source_checkpoint = prepared.source_checkpoint;
        metadata.source_position = prepared.source_position;
        metadata.last_applied_event_id = prepared.last_applied_event_id;
        metadata.applied_event_count = metadata
            .applied_event_count
            .saturating_add(prepared.applied);
        metadata.skipped_duplicate_count = metadata
            .skipped_duplicate_count
            .saturating_add(prepared.skipped);
        metadata.lag = batch.lag;
        metadata.freshness = if batch.lag == 0 {
            crate::catalog::ProjectionFreshness::Fresh
        } else {
            crate::catalog::ProjectionFreshness::Stale
        };
        metadata.last_error = None;
        let failure_expected = stored_metadata.clone();
        let replay = ReplayBatchCommit {
            expected_metadata: stored_metadata,
            metadata: metadata.clone(),
            source_identity: batch.source_identity.clone(),
            batch_id: batch.batch_id.clone(),
            observed_events: prepared.observed_events,
            new_event_ids: prepared.replay_event_ids,
        };
        let rollups = self.catalog.list_rollups_for_source(&batch.projection);
        let record_rollup_maintenance_debt = !rollups.is_empty();
        let mut rollup_fields = BTreeSet::new();
        for rollup in rollups {
            rollup_fields.insert(rollup.timestamp_field);
            rollup_fields.extend(rollup.group_keys);
        }
        let result = self.midge.apply_projection_replay_batch(
            &batch.projection,
            std::mem::take(&mut prepared.write_ops),
            replay,
            commit,
            rollup_fields.into_iter().collect(),
            record_rollup_maintenance_debt,
        );
        let write_report = match result {
            Ok(report) => report,
            // Interrupted before native submission: nothing was published and
            // this is not a projection failure, so no diagnostic is persisted
            // with the already-interrupted authority.
            Err(error @ (CassieError::QueryCanceled | CassieError::DeadlineExceeded)) => {
                return Err(error);
            }
            Err(CassieError::StorageRetryable(error))
                if error.kind() == super::StorageRetryKind::WriteStall =>
            {
                let message = format!("projection replay failed to apply events: {error}");
                let _ = self.fail_replay_metadata::<()>(
                    previous_metadata,
                    failure_expected.as_ref(),
                    &batch,
                    message,
                );
                return Err(CassieError::StorageRetryable(error));
            }
            Err(
                error @ (CassieError::Storage(_)
                | CassieError::StorageBootstrap(_)
                | CassieError::StorageMissingFamily(_)
                | CassieError::StorageRetryable(_)),
            ) => return Err(error),
            Err(error) => {
                return self.fail_replay_metadata(
                    previous_metadata,
                    failure_expected.as_ref(),
                    &batch,
                    format!("projection replay failed to apply events: {error}"),
                );
            }
        };
        // The publication is confirmed; a later cancellation cannot undo it.
        let _terminal = PublicationControlScope::terminal_completion();
        if let Some(epoch) = write_report.data_epoch {
            self.runtime.set_data_epoch(epoch);
        }
        if record_rollup_maintenance_debt
            && (!write_report.rollup_changes.rows.is_empty()
                || write_report.rollup_changes.exceeded_limit)
        {
            let controls = self.runtime.query_controls(std::time::Instant::now());
            let _ = crate::executor::refresh_rollups_for_source_with_report_external(
                self,
                &batch.projection,
                &write_report.rollup_changes,
                &controls,
            );
        }
        let mut stats = write_report.stats;
        stats.duplicate_checks = prepared.duplicate_checks;
        if stats.duplicate_checks > 0 || prepared.applied > 0 {
            self.runtime
                .record_projection_write_batch(batch.projection.clone(), &stats);
        }
        #[cfg(test)]
        if POSTCOMMIT_READ_FAILURE.with(|flag| flag.replace(false)) {
            return Err(CassieError::Storage(
                "injected replay metadata read failure after commit".to_owned(),
            ));
        }
        if let Some(latest) = self.midge.projection_metadata(&metadata.collection)? {
            metadata = latest;
        }
        self.catalog.register_projection_metadata(metadata.clone());
        self.runtime
            .record_projection_replay(batch.projection, prepared.applied, prepared.skipped);
        crate::midge::adapter::check_projection_replay_post_commit_failure_point()?;

        Ok(ProjectionReplayReport {
            applied_event_count: prepared.applied,
            skipped_duplicate_count: prepared.skipped,
            freshness: metadata.freshness,
            source_checkpoint: metadata.source_checkpoint,
            last_applied_event_id: metadata.last_applied_event_id,
        })
    }

    fn fail_replay_metadata<T>(
        &self,
        mut metadata: crate::catalog::ProjectionMeta,
        expected: Option<&crate::catalog::ProjectionMeta>,
        batch: &ProjectionReplayBatch,
        message: String,
    ) -> Result<T, CassieError> {
        if metadata.source_identity.is_none() {
            metadata.source_identity = Some(batch.source_identity.clone());
        }
        metadata.replay_batch_id = Some(batch.batch_id.clone());
        metadata.freshness = crate::catalog::ProjectionFreshness::Failed;
        metadata.last_error = Some(message.clone());
        self.midge
            .put_replay_failure_if_unchanged(expected, &metadata)?;
        self.catalog.register_projection_metadata(metadata);
        self.runtime
            .record_projection_replay_error(batch.projection.clone(), message.clone());
        Err(CassieError::Execution(message))
    }
}

#[cfg(test)]
#[path = "replay_cancellation_tests.rs"]
mod cancellation_tests;
