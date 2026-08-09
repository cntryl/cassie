use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use cassie::app::{
    projection_concurrency_test_guard, set_projection_replay_prepare_barriers, Cassie,
    ProjectionReplayBatch, ProjectionReplayEvent,
};
use cassie::executor::{
    set_materialized_projection_replace_barriers,
    set_materialized_projection_replace_start_barriers,
};
use cassie::types::Value;
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
    std::env::temp_dir()
        .join(format!(
            "cassie-projection-concurrency-{label}-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .into_owned()
}

fn replay_batch(
    projection: &str,
    batch_id: &str,
    event_id: &str,
    checkpoint: &str,
    position: u64,
) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_string(),
        source_identity: "projection-concurrency-source".to_string(),
        batch_id: batch_id.to_string(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: event_id.to_string(),
            checkpoint: checkpoint.to_string(),
            position: Some(position),
            document_id: event_id.to_string(),
            payload: Some(serde_json::json!({"marker": checkpoint})),
        }],
    }
}

fn canonical_projection(cassie: &Cassie, collection: &str) -> String {
    cassie
        .catalog
        .get_schema(collection)
        .map_or_else(|| collection.to_string(), |schema| schema.collection)
}

#[test]
fn should_serialize_concurrent_materialized_projection_refreshes() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let path = data_dir("refresh");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE projection_concurrent_source (title TEXT, score INT)",
            vec![],
        )
        .expect("create source");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO projection_concurrent_source VALUES ('alpha', 1), ('bravo', 2)",
            vec![],
        )
        .expect("seed source");
    cassie
        .execute_sql(
            &setup,
            "CREATE MATERIALIZED PROJECTION projection_concurrent AS SELECT title FROM projection_concurrent_source WHERE score > 1",
            vec![],
        )
        .expect("create projection");
    let dropped = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let replace_ready = Arc::new(Barrier::new(2));
    let replace_resume = Arc::new(Barrier::new(2));
    set_materialized_projection_replace_start_barriers(
        Some(Arc::clone(&replace_ready)),
        Some(Arc::clone(&replace_resume)),
    );
    set_materialized_projection_replace_barriers(
        Some(Arc::clone(&dropped)),
        Some(Arc::clone(&resume)),
    );

    // Act
    let first_cassie = Arc::clone(&cassie);
    let first = std::thread::spawn(move || {
        let session = first_cassie.create_session("first", None);
        first_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED PROJECTION projection_concurrent",
            vec![],
        )
    });
    replace_ready.wait();
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO projection_concurrent_source VALUES ('charlie', 3)",
            vec![],
        )
        .expect("mutate source between refresh snapshots");
    replace_resume.wait();
    dropped.wait();
    let (second_tx, second_rx) = mpsc::channel();
    let second_cassie = Arc::clone(&cassie);
    let second = std::thread::spawn(move || {
        let session = second_cassie.create_session("second", None);
        let result = second_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED PROJECTION projection_concurrent",
            vec![],
        );
        second_tx.send(result).expect("send second refresh result");
    });
    let early_second = second_rx.recv_timeout(Duration::from_millis(250)).ok();
    resume.wait();
    let first_result = first.join().expect("first refresh thread");
    second.join().expect("second refresh thread");
    let second_result = early_second.unwrap_or_else(|| {
        second_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second refresh should complete")
    });
    let rows = cassie
        .execute_sql(
            &setup,
            "SELECT title FROM projection_concurrent ORDER BY title",
            vec![],
        )
        .expect("query refreshed projection");

    // Assert
    first_result.expect("first refresh should succeed");
    second_result.expect("second refresh should succeed");
    assert_eq!(
        rows.rows,
        vec![
            vec![Value::String("bravo".to_string())],
            vec![Value::String("charlie".to_string())],
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_concurrent_replay_checkpoint_monotonic() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let path = data_dir("replay-checkpoint");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE projection_checkpoint_target (marker TEXT)",
            vec![],
        )
        .expect("create replay target");
    let projection = canonical_projection(&cassie, "projection_checkpoint_target");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_projection_replay_prepare_barriers(
        Some("batch-low".to_string()),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );

    // Act
    let low_cassie = Arc::clone(&cassie);
    let low_projection = projection.clone();
    let low = std::thread::spawn(move || {
        low_cassie.replay_projection_batch(replay_batch(
            &low_projection,
            "batch-low",
            "event-low",
            "checkpoint-5",
            5,
        ))
    });
    ready.wait();
    let (high_tx, high_rx) = mpsc::channel();
    let high_cassie = Arc::clone(&cassie);
    let high_projection = projection.clone();
    let high = std::thread::spawn(move || {
        let result = high_cassie.replay_projection_batch(replay_batch(
            &high_projection,
            "batch-high",
            "event-high",
            "checkpoint-10",
            10,
        ));
        high_tx.send(result).expect("send high replay result");
    });
    let early_high = high_rx.recv_timeout(Duration::from_millis(250)).ok();
    resume.wait();
    low.join().expect("low replay thread").expect("low replay");
    high.join().expect("high replay thread");
    early_high
        .unwrap_or_else(|| {
            high_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("high replay should complete")
        })
        .expect("high replay");
    let metadata = cassie
        .catalog
        .get_projection_metadata(&projection)
        .expect("projection metadata");

    // Assert
    assert_eq!(metadata.source_position, Some(10));
    assert_eq!(metadata.source_checkpoint.as_deref(), Some("checkpoint-10"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_concurrent_duplicate_replay_event_once() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let path = data_dir("replay-duplicate");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE projection_duplicate_target (marker TEXT)",
            vec![],
        )
        .expect("create replay target");
    let projection = canonical_projection(&cassie, "projection_duplicate_target");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_projection_replay_prepare_barriers(
        Some("batch-paused".to_string()),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );

    // Act
    let paused_cassie = Arc::clone(&cassie);
    let paused_projection = projection.clone();
    let paused = std::thread::spawn(move || {
        paused_cassie.replay_projection_batch(replay_batch(
            &paused_projection,
            "batch-paused",
            "event-shared",
            "checkpoint-1",
            1,
        ))
    });
    ready.wait();
    let (racing_tx, racing_rx) = mpsc::channel();
    let racing_cassie = Arc::clone(&cassie);
    let racing_projection = projection.clone();
    let racing = std::thread::spawn(move || {
        let result = racing_cassie.replay_projection_batch(replay_batch(
            &racing_projection,
            "batch-racing",
            "event-shared",
            "checkpoint-1",
            1,
        ));
        racing_tx.send(result).expect("send racing replay result");
    });
    let early_racing = racing_rx.recv_timeout(Duration::from_millis(250)).ok();
    resume.wait();
    let paused = paused
        .join()
        .expect("paused replay thread")
        .expect("paused replay");
    racing.join().expect("racing replay thread");
    let racing = early_racing
        .unwrap_or_else(|| {
            racing_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("racing replay should complete")
        })
        .expect("racing replay");

    // Assert
    assert_eq!(paused.applied_event_count + racing.applied_event_count, 1);
    assert_eq!(
        paused.skipped_duplicate_count + racing.skipped_duplicate_count,
        1
    );
    let _ = std::fs::remove_dir_all(path);
}
