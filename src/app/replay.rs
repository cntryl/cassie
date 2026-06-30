use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::midge::adapter::DocumentWriteOp;

use super::{Cassie, CassieError};

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
    source_checkpoint: Option<String>,
    source_position: Option<u64>,
    last_applied_event_id: Option<String>,
}

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn replay_projection_batch(
        &self,
        batch: ProjectionReplayBatch,
    ) -> Result<ProjectionReplayReport, CassieError> {
        Self::validate_replay_batch(&batch)?;
        let metadata = self
            .load_projection_replay_metadata(&batch)
            .and_then(|metadata| self.ensure_replay_source_identity(metadata, &batch))?;
        let mut prepared = self.prepare_projection_replay(&metadata, &batch)?;
        self.apply_projection_replay_writes(&metadata, &batch, &mut prepared)?;
        self.finalize_projection_replay(metadata, batch, prepared)
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

    fn load_projection_replay_metadata(
        &self,
        batch: &ProjectionReplayBatch,
    ) -> Result<crate::catalog::ProjectionMeta, CassieError> {
        self.catalog
            .get_projection_metadata(&batch.projection)
            .or_else(|| {
                self.midge
                    .projection_metadata(&batch.projection)
                    .ok()
                    .flatten()
            })
            .ok_or_else(|| CassieError::CollectionNotFound(batch.projection.clone()))
    }

    fn ensure_replay_source_identity(
        &self,
        metadata: crate::catalog::ProjectionMeta,
        batch: &ProjectionReplayBatch,
    ) -> Result<crate::catalog::ProjectionMeta, CassieError> {
        if let Some(existing) = metadata.source_identity.clone() {
            if existing != batch.source_identity {
                return self.fail_replay_metadata(
                    metadata,
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

        for (event, already_seen) in batch.events.iter().zip(event_seen) {
            duplicate_checks = duplicate_checks.saturating_add(1);
            if already_seen {
                skipped = skipped.saturating_add(1);
                continue;
            }
            if !batch_event_ids.insert(event.event_id.clone()) {
                return self.fail_replay_metadata(
                    metadata.clone(),
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
            source_checkpoint,
            source_position,
            last_applied_event_id,
        })
    }

    fn apply_projection_replay_writes(
        &self,
        metadata: &crate::catalog::ProjectionMeta,
        batch: &ProjectionReplayBatch,
        prepared: &mut PreparedReplay,
    ) -> Result<(), CassieError> {
        let write_ops = std::mem::take(&mut prepared.write_ops);
        if write_ops.is_empty() {
            let write_stats = crate::runtime::ProjectionWriteStats {
                duplicate_checks: prepared.duplicate_checks,
                ..Default::default()
            };
            if prepared.duplicate_checks > 0 {
                self.runtime
                    .record_projection_write_batch(batch.projection.clone(), &write_stats);
            }
            return Ok(());
        }

        let write_report = self
            .midge
            .apply_document_write_batch(&batch.projection, write_ops)
            .map_err(|error| {
                CassieError::Execution(format!("projection replay failed to apply events: {error}"))
            });
        let write_report = match write_report {
            Ok(report) => report,
            Err(CassieError::Execution(message)) => {
                return self.fail_replay_metadata(metadata.clone(), batch, message);
            }
            Err(error) => return Err(error),
        };
        self.record_projection_replay_events(metadata, batch, &prepared.replay_event_ids)?;
        let mut write_stats = write_report.stats;
        write_stats.duplicate_checks = prepared.duplicate_checks;
        self.runtime
            .record_projection_write_batch(batch.projection.clone(), &write_stats);
        Ok(())
    }

    fn record_projection_replay_events(
        &self,
        metadata: &crate::catalog::ProjectionMeta,
        batch: &ProjectionReplayBatch,
        replay_event_ids: &[String],
    ) -> Result<(), CassieError> {
        let replay_event_refs = replay_event_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if replay_event_refs.is_empty() {
            return Ok(());
        }
        if let Err(error) = self.midge.record_projection_events_batch(
            &batch.projection,
            &batch.source_identity,
            &replay_event_refs,
            &batch.batch_id,
        ) {
            return self.fail_replay_metadata(
                metadata.clone(),
                batch,
                format!("projection replay failed to record duplicate ledger: {error}"),
            );
        }
        Ok(())
    }

    fn finalize_projection_replay(
        &self,
        mut metadata: crate::catalog::ProjectionMeta,
        batch: ProjectionReplayBatch,
        prepared: PreparedReplay,
    ) -> Result<ProjectionReplayReport, CassieError> {
        metadata.source_identity = Some(batch.source_identity);
        metadata.replay_batch_id = Some(batch.batch_id);
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
        self.persist_replay_metadata(metadata.clone())?;
        self.runtime
            .record_projection_replay(batch.projection, prepared.applied, prepared.skipped);

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
        batch: &ProjectionReplayBatch,
        message: String,
    ) -> Result<T, CassieError> {
        metadata.source_identity = Some(batch.source_identity.clone());
        metadata.replay_batch_id = Some(batch.batch_id.clone());
        metadata.freshness = crate::catalog::ProjectionFreshness::Failed;
        metadata.last_error = Some(message.clone());
        self.persist_replay_metadata(metadata)?;
        self.runtime
            .record_projection_replay_error(batch.projection.clone(), message.clone());
        Err(CassieError::Execution(message))
    }

    fn persist_replay_metadata(
        &self,
        metadata: crate::catalog::ProjectionMeta,
    ) -> Result<(), CassieError> {
        self.midge.put_projection_metadata(&metadata)?;
        self.catalog.register_projection_metadata(metadata);
        Ok(())
    }
}
