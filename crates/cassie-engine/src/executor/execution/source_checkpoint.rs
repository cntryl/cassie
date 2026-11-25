//! Snapshot-consistent checkpoint rows with retained query-memory ownership.
use std::sync::Arc;

use super::{batch, Batch, BatchRow, QueryError, SourceExecutionEnv};
use crate::runtime::QueryMemoryReservation;
use crate::types::Value;

fn text(name: &str, value: String) -> (String, Value) {
    (name.to_owned(), Value::String(value))
}

fn integer(name: &str, value: u64) -> (String, Value) {
    (
        name.to_owned(),
        Value::Int64(i64::try_from(value).unwrap_or(i64::MAX)),
    )
}

pub(super) fn materialize(env: &SourceExecutionEnv<'_>) -> Result<Vec<Batch>, QueryError> {
    let database = env
        .session
        .and_then(|session| session.current_database())
        .unwrap_or(&env.cassie.default_database);
    let checkpoints = env
        .cassie
        .midge
        .checkpoint_rows_for_statement(database, &env.controls.storage_request())
        .map_err(QueryError::from)?;
    let mut rows = Vec::new();
    for (checkpoint, memory) in checkpoints {
        env.controls
            .check_interruption()
            .map_err(QueryError::from)?;
        rows.try_reserve(1).map_err(|error| {
            QueryError::from(crate::runtime::owned_memory::allocation_error(&error))
        })?;
        let projection_id = if checkpoint.projection_id.is_empty() {
            checkpoint.collection.clone()
        } else {
            checkpoint.projection_id
        };
        let fields = vec![
            text("projection_id", projection_id),
            text("collection", checkpoint.collection),
            text("kind", checkpoint.kind.as_str().to_owned()),
            text(
                "source_identity",
                checkpoint.source_identity.unwrap_or_default(),
            ),
            text(
                "source_checkpoint",
                checkpoint.source_checkpoint.unwrap_or_default(),
            ),
            integer(
                "source_position",
                checkpoint.source_position.unwrap_or_default(),
            ),
            text(
                "last_applied_event_id",
                checkpoint.last_applied_event_id.unwrap_or_default(),
            ),
            text(
                "replay_batch_id",
                checkpoint.replay_batch_id.unwrap_or_default(),
            ),
            integer("lag", checkpoint.lag),
            text("freshness", checkpoint.freshness.as_str().to_owned()),
            text("last_error", checkpoint.last_error.unwrap_or_default()),
        ];
        rows.push(
            BatchRow::new(fields)
                .with_query_memory(Some(Arc::new(QueryMemoryReservation::from_core(memory)))),
        );
    }
    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}
