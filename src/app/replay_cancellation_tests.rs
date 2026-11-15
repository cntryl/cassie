//! Cancellation boundaries of the cancellable projection replay entrypoints.

use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use crate::app::{
    Cassie, CassieError, ProjectionReplayBatch, ProjectionReplayEvent, QueryCancellationHandle,
};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, PublicationControlScope, SchemaWritePausePoint,
};

const HANG_GUARD: Duration = Duration::from_secs(30);

fn fixture(table: &str) -> (Arc<Cassie>, String) {
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("projector", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (title TEXT)"),
            vec![],
        )
        .unwrap();
    let projection = cassie.midge.canonical_collection_name(table);
    (cassie, projection)
}

fn batch(projection: &str) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_owned(),
        source_identity: "orders".to_owned(),
        batch_id: "batch-1".to_owned(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-1".to_owned(),
            checkpoint: "checkpoint-1".to_owned(),
            position: Some(1),
            document_id: "doc-1".to_owned(),
            payload: Some(serde_json::json!({"title": "replayed"})),
        }],
    }
}

/// Row presence, ledger entry, stored checkpoint and stored failure diagnostic.
fn replay_state(cassie: &Cassie, projection: &str) -> (bool, bool, Option<String>, Option<String>) {
    let row = cassie
        .midge
        .get_document(projection, "doc-1")
        .unwrap()
        .is_some();
    let event = cassie
        .midge
        .has_projection_event(projection, "orders", "event-1")
        .unwrap();
    let metadata = cassie.midge.projection_metadata(projection).unwrap();
    let checkpoint = metadata
        .as_ref()
        .and_then(|metadata| metadata.source_checkpoint.clone());
    let last_error = metadata.and_then(|metadata| metadata.last_error);
    (row, event, checkpoint, last_error)
}

#[test]
fn should_leave_projection_unchanged_when_canceled_at_preflight() {
    // Arrange
    let (cassie, projection) = fixture("replay_cancel_preflight");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = cassie.replay_projection_batch_cancellable(batch(&projection), &cancellation);
    let state = replay_state(&cassie, &projection);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(state, (false, false, None, None));
}

#[test]
fn should_stop_waiting_for_the_collection_gate_when_canceled() {
    // Arrange
    let (cassie, projection) = fixture("replay_cancel_gate");
    let cancellation = QueryCancellationHandle::new();
    let gate = cassie.midge.collection_write_gate(&projection);
    let held = gate.lock();
    let (done_tx, done_rx) = mpsc::channel();
    let worker_cassie = Arc::clone(&cassie);
    let worker_cancellation = cancellation.clone();
    let worker_batch = batch(&projection);
    std::thread::spawn(move || {
        let result =
            worker_cassie.replay_projection_batch_cancellable(worker_batch, &worker_cancellation);
        done_tx.send(result).unwrap();
    });

    // Act
    cancellation.cancel();
    let result = done_rx.recv_timeout(HANG_GUARD).unwrap();
    drop(held);
    let state = replay_state(&cassie, &projection);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(state, (false, false, None, None));
}

#[test]
fn should_abort_without_publication_when_canceled_after_staging() {
    // Arrange
    let _guard = schema_write_conflict_test_guard();
    let (cassie, projection) = fixture("replay_cancel_staged");
    let cancellation = QueryCancellationHandle::new();
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::ProjectionReplayMetadataCommit),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let worker_cassie = Arc::clone(&cassie);
    let worker_cancellation = cancellation.clone();
    let worker_batch = batch(&projection);
    let worker = std::thread::spawn(move || {
        let _worker_guard = schema_write_conflict_worker_guard();
        worker_cassie.replay_projection_batch_cancellable(worker_batch, &worker_cancellation)
    });
    ready.wait();

    // Act
    set_schema_write_commit_barriers(None, None, None);
    cancellation.cancel();
    resume.wait();
    let canceled = worker.join().unwrap();
    let state_after_cancel = replay_state(&cassie, &projection);
    let retry = cassie
        .replay_projection_batch_cancellable(batch(&projection), &QueryCancellationHandle::new())
        .unwrap();
    let state_after_retry = replay_state(&cassie, &projection);

    // Assert
    assert!(matches!(canceled, Err(CassieError::QueryCanceled)));
    assert_eq!(state_after_cancel, (false, false, None, None));
    assert_eq!(retry.applied_event_count, 1);
    assert_eq!(retry.skipped_duplicate_count, 0);
    assert_eq!(
        state_after_retry,
        (true, true, Some("checkpoint-1".to_owned()), None)
    );
}

#[test]
fn should_report_success_when_canceled_after_native_commit_confirmation() {
    // Arrange
    let (cassie, projection) = fixture("replay_cancel_confirmed");
    let cancellation = QueryCancellationHandle::new();
    let confirmed = cancellation.clone();
    let _hook = crate::midge::adapter::Midge::observe_next_row_commit_for_test(move || {
        confirmed.cancel();
    });

    // Act
    let result =
        cassie.replay_projection_batch_strict_cancellable(batch(&projection), &cancellation);
    let state = replay_state(&cassie, &projection);
    let catalog_checkpoint = cassie
        .catalog
        .get_projection_metadata(&projection)
        .and_then(|metadata| metadata.source_checkpoint);

    // Assert
    assert!(cancellation.is_canceled());
    assert_eq!(result.unwrap().applied_event_count, 1);
    assert_eq!(state, (true, true, Some("checkpoint-1".to_owned()), None));
    assert_eq!(catalog_checkpoint.as_deref(), Some("checkpoint-1"));
}

#[test]
fn should_report_unknown_outcome_instead_of_cancellation_after_submission() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-replay-cancel-unknown-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("projector", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE replay_cancel_unknown (title TEXT)",
            vec![],
        )
        .unwrap();
    let projection = cassie
        .midge
        .canonical_collection_name("replay_cancel_unknown");
    let cancellation = QueryCancellationHandle::new();
    let confirmed = cancellation.clone();
    let _hook = crate::midge::adapter::Midge::observe_next_row_commit_for_test(move || {
        confirmed.cancel();
    });
    crate::midge::adapter::inject_native_commit_unknown_after_success();

    // Act
    let result =
        cassie.replay_projection_batch_strict_cancellable(batch(&projection), &cancellation);
    let fenced = cassie.midge.projection_metadata(&projection);
    cassie.shutdown();
    drop(cassie);
    let recovered =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    recovered.startup().unwrap();
    let duplicate = recovered
        .replay_projection_batch_cancellable(batch(&projection), &QueryCancellationHandle::new())
        .unwrap();
    recovered.shutdown();
    drop(recovered);
    std::fs::remove_dir_all(path).unwrap();

    // Assert
    assert!(result.is_err());
    assert!(!matches!(result, Err(CassieError::QueryCanceled)));
    assert!(fenced.is_err());
    assert_eq!(duplicate.applied_event_count, 0);
    assert_eq!(duplicate.skipped_duplicate_count, 1);
}

#[test]
fn should_keep_ambient_cancellation_when_explicit_handle_is_live() {
    // Arrange
    let (cassie, projection) = fixture("replay_cancel_ambient");
    let ambient = QueryCancellationHandle::new();
    let _ambient_scope = PublicationControlScope::enter_for_replay(&ambient);
    ambient.cancel();

    // Act
    let result = cassie
        .replay_projection_batch_cancellable(batch(&projection), &QueryCancellationHandle::new());
    let state = replay_state(&cassie, &projection);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(state, (false, false, None, None));
}

#[test]
fn should_leave_uncontrolled_replay_unaffected_by_cancellable_entrypoints() {
    // Arrange
    let (cassie, projection) = fixture("replay_cancel_legacy");

    // Act
    let report = cassie.replay_projection_batch(batch(&projection)).unwrap();
    let state = replay_state(&cassie, &projection);

    // Assert
    assert_eq!(report.applied_event_count, 1);
    assert_eq!(state, (true, true, Some("checkpoint-1".to_owned()), None));
}

#[test]
fn should_stop_uncontrolled_replay_at_an_expired_ambient_deadline() {
    // Arrange
    let (cassie, projection) = fixture("replay_cancel_deadline");
    let ambient = QueryCancellationHandle::new();
    let _ambient_scope =
        PublicationControlScope::enter_for_rest(&ambient, std::time::Instant::now());

    // Act
    let result = cassie.replay_projection_batch(batch(&projection));
    let state = replay_state(&cassie, &projection);

    // Assert
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    assert_eq!(state, (false, false, None, None));
}
