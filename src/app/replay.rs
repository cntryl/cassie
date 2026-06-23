use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::midge::adapter::DocumentWriteOp;

use super::*;

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

impl Cassie {
    pub fn replay_projection_batch(
        &self,
        batch: ProjectionReplayBatch,
    ) -> Result<ProjectionReplayReport, CassieError> {
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
        let mut metadata = self
            .catalog
            .get_projection_metadata(&batch.projection)
            .or_else(|| {
                self.midge
                    .projection_metadata(&batch.projection)
                    .ok()
                    .flatten()
            })
            .ok_or_else(|| CassieError::CollectionNotFound(batch.projection.clone()))?;

        if let Some(existing) = metadata.source_identity.clone() {
            if existing != batch.source_identity {
                return self.fail_replay_metadata(
                    metadata,
                    &batch,
                    format!(
                        "projection '{}' is bound to source '{existing}', not '{}'",
                        batch.projection, batch.source_identity
                    ),
                );
            }
        }

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
                    metadata,
                    &batch,
                    "projection replay event id cannot be empty".to_string(),
                );
            }
            duplicate_checks = duplicate_checks.saturating_add(1);
            if self.midge.has_projection_event(
                &batch.projection,
                &batch.source_identity,
                &event.event_id,
            )? {
                skipped = skipped.saturating_add(1);
                continue;
            }
            if !batch_event_ids.insert(event.event_id.clone()) {
                return self.fail_replay_metadata(
                    metadata,
                    &batch,
                    format!(
                        "duplicate projection replay event '{}' in batch '{}'",
                        event.event_id, batch.batch_id
                    ),
                );
            }
            if let (Some(previous), Some(next)) = (position_cursor, event.position) {
                if next <= previous {
                    return self.fail_replay_metadata(
                        metadata,
                        &batch,
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

        if !write_ops.is_empty() {
            let write_report = match self
                .midge
                .apply_document_write_batch(&batch.projection, write_ops)
            {
                Ok(report) => report,
                Err(error) => {
                    return self.fail_replay_metadata(
                        metadata,
                        &batch,
                        format!("projection replay failed to apply events: {error}"),
                    );
                }
            };
            let replay_event_refs = replay_event_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            if !replay_event_refs.is_empty() {
                if let Err(error) = self.midge.record_projection_events_batch(
                    &batch.projection,
                    &batch.source_identity,
                    &replay_event_refs,
                    &batch.batch_id,
                ) {
                    return self.fail_replay_metadata(
                        metadata,
                        &batch,
                        format!("projection replay failed to record duplicate ledger: {error}"),
                    );
                }
            }

            let mut write_stats = write_report.stats;
            write_stats.duplicate_checks = duplicate_checks;
            self.runtime
                .record_projection_write_batch(batch.projection.to_string(), &write_stats);
        } else {
            let write_stats = crate::runtime::ProjectionWriteStats {
                duplicate_checks,
                ..Default::default()
            };
            if duplicate_checks > 0 {
                self.runtime
                    .record_projection_write_batch(batch.projection.to_string(), &write_stats);
            }
        }

        metadata.source_identity = Some(batch.source_identity);
        metadata.replay_batch_id = Some(batch.batch_id);
        metadata.source_checkpoint = source_checkpoint;
        metadata.source_position = source_position;
        metadata.last_applied_event_id = last_applied_event_id;
        metadata.applied_event_count = metadata.applied_event_count.saturating_add(applied);
        metadata.skipped_duplicate_count = metadata.skipped_duplicate_count.saturating_add(skipped);
        metadata.lag = batch.lag;
        metadata.freshness = if batch.lag == 0 {
            crate::catalog::ProjectionFreshness::Fresh
        } else {
            crate::catalog::ProjectionFreshness::Stale
        };
        metadata.last_error = None;
        self.persist_replay_metadata(metadata.clone())?;
        self.runtime
            .record_projection_replay(batch.projection, applied, skipped);

        Ok(ProjectionReplayReport {
            applied_event_count: applied,
            skipped_duplicate_count: skipped,
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
        self.midge.put_projection_metadata(metadata.clone())?;
        self.catalog.register_projection_metadata(metadata);
        Ok(())
    }
}
