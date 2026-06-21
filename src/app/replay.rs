use serde::{Deserialize, Serialize};

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
        for event in &batch.events {
            if event.event_id.trim().is_empty() {
                return self.fail_replay_metadata(
                    metadata,
                    &batch,
                    "projection replay event id cannot be empty".to_string(),
                );
            }
            if self.midge.has_projection_event(
                &batch.projection,
                &batch.source_identity,
                &event.event_id,
            )? {
                skipped = skipped.saturating_add(1);
                continue;
            }
            if let (Some(previous), Some(next)) = (metadata.source_position, event.position) {
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

            let write_result = if let Some(payload) = &event.payload {
                self.midge
                    .put_document(
                        &batch.projection,
                        Some(event.document_id.clone()),
                        payload.clone(),
                    )
                    .map(|_| ())
            } else {
                self.midge
                    .delete_document(&batch.projection, &event.document_id)
                    .map(|_| ())
            };
            if let Err(error) = write_result {
                return self.fail_replay_metadata(
                    metadata,
                    &batch,
                    format!(
                        "projection replay failed on event '{}': {error}",
                        event.event_id
                    ),
                );
            }

            self.midge.record_projection_event(
                &batch.projection,
                &batch.source_identity,
                &event.event_id,
                &batch.batch_id,
            )?;
            applied = applied.saturating_add(1);
            metadata.source_checkpoint = Some(event.checkpoint.clone());
            metadata.source_position = event.position;
            metadata.last_applied_event_id = Some(event.event_id.clone());
        }

        metadata.source_identity = Some(batch.source_identity);
        metadata.replay_batch_id = Some(batch.batch_id);
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
