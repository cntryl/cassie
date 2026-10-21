#[test]
fn should_replay_projection_batch_idempotently_with_checkpoint_metadata() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_replay_idempotent");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_replay_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_docs");
        let batch = ProjectionReplayBatch {
            projection: projection.clone(),
            source_identity: "orders-stream".to_string(),
            batch_id: "batch-1".to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "event-1".to_string(),
                checkpoint: "checkpoint-1".to_string(),
                position: Some(1),
                document_id: "doc-1".to_string(),
                payload: Some(serde_json::json!({"title": "alpha"})),
            }],
        };

        // Act
        let first = cassie.replay_projection_batch(batch.clone()).unwrap();
        let duplicate = cassie.replay_projection_batch(batch).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT source_identity, source_checkpoint, last_applied_event_id, replay_batch_id, freshness FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.applied_event_count, 1);
        assert_eq!(duplicate.skipped_duplicate_count, 1);
        assert_eq!(selected.rows(), vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(
            checkpoint.rows(),
            vec![vec![
                Value::String("orders-stream".to_string()),
                Value::String("checkpoint-1".to_string()),
                Value::String("event-1".to_string()),
                Value::String("batch-1".to_string()),
                Value::String("fresh".to_string()),
            ]]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["projections"]["replay_batches"].as_u64(), Some(2));
        assert_eq!(
            metrics["projections"]["replay_duplicates_skipped"].as_u64(),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

fn start_projection_replay_retry_fixture(
    label: &str,
) -> (tokio::runtime::Runtime, String, Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let (cassie, session, projection) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_replay_schema_retry_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection =
            canonical_test_collection(&cassie, "projection_replay_schema_retry_docs");
        (cassie, session, projection)
    });
    (runtime, path, cassie, session, projection)
}

fn projection_replay_retry_batch(projection: &str) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_string(),
        source_identity: "orders-stream".to_string(),
        batch_id: "batch-1".to_string(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-1".to_string(),
            checkpoint: "checkpoint-1".to_string(),
            position: Some(1),
            document_id: "doc-1".to_string(),
            payload: Some(serde_json::json!({"title": "alpha"})),
        }],
    }
}

#[test]
fn should_retry_projection_replay_when_metadata_commit_fails_after_row_commit() {
    // Arrange
    let (_runtime, path, cassie, session, projection) =
        start_projection_replay_retry_fixture("projection_replay_metadata_commit_retry");
    let batch = projection_replay_retry_batch(&projection);
    set_projection_replay_metadata_commit_failure_point(true);

    // Act
    let failed = cassie.replay_projection_batch(batch.clone());
    set_projection_replay_metadata_commit_failure_point(false);
    let rows_after_failed_metadata_commit = query_rows(
        &cassie,
        &session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );
    let event_seen_after_failed_metadata_commit = cassie
        .midge
        .has_projection_event(&projection, "orders-stream", "event-1")
        .unwrap();
    let metadata_after_failed_metadata_commit = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();

    // Assert
    assert!(failed.is_err(), "metadata failure was not returned: {failed:?}");
    assert_eq!(
        rows_after_failed_metadata_commit,
        vec![vec![Value::String("alpha".to_string())]]
    );
    assert!(!event_seen_after_failed_metadata_commit);
    assert_eq!(metadata_after_failed_metadata_commit.source_checkpoint, None);
    assert_eq!(metadata_after_failed_metadata_commit.last_applied_event_id, None);

    let retry = cassie.replay_projection_batch(batch).unwrap();
    let rows_after_retry = query_rows(
        &cassie,
        &session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );
    let metadata_after_retry = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let event_seen_after_retry = cassie
        .midge
        .has_projection_event(&projection, "orders-stream", "event-1")
        .unwrap();

    // Assert
    assert_eq!(retry.applied_event_count, 1);
    assert_eq!(rows_after_retry, rows_after_failed_metadata_commit);
    assert!(event_seen_after_retry);
    assert_eq!(
        metadata_after_retry.source_checkpoint.as_deref(),
        Some("checkpoint-1")
    );
    assert_eq!(
        metadata_after_retry.last_applied_event_id.as_deref(),
        Some("event-1")
    );

    cassie.shutdown();
    drop(cassie);
    let recovered = Cassie::new_with_data_dir(&path).unwrap();
    recovered.startup().unwrap();
    let recovered_session = recovered.create_session("tester", None);
    let recovered_rows = query_rows(
        &recovered,
        &recovered_session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );
    let recovered_metadata = recovered
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let recovered_event_seen = recovered
        .midge
        .has_projection_event(&projection, "orders-stream", "event-1")
        .unwrap();

    // Assert
    assert_eq!(recovered_rows, rows_after_retry);
    assert!(recovered_event_seen);
    assert_eq!(
        recovered_metadata.source_checkpoint.as_deref(),
        Some("checkpoint-1")
    );

    recovered.shutdown();
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_retry_projection_replay_after_metadata_commit_conflict() {
    // Arrange
    let _schema_write_guard = schema_write_conflict_test_guard();
    let (_runtime, path, cassie, session, projection) =
        start_projection_replay_retry_fixture("projection_replay_metadata_commit_conflict");
    let cassie = Arc::new(cassie);
    let batch = projection_replay_retry_batch(&projection);
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::ProjectionReplayMetadataCommit),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let replay_cassie = Arc::clone(&cassie);
    let replay_batch = batch.clone();
    let replay = std::thread::spawn(move || {
        let _worker_guard = schema_write_conflict_worker_guard();
        replay_cassie.replay_projection_batch(replay_batch)
    });
    ready.wait();

    // Act
    set_schema_write_commit_barriers(None, None, None);
    let mut competing_metadata = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    competing_metadata.last_error = Some("concurrent schema writer".to_string());
    cassie
        .midge
        .put_projection_metadata(&competing_metadata)
        .unwrap();
    resume.wait();
    let failed = replay.join().expect("join replay worker");
    let rows_after_conflict = query_rows(
        &cassie,
        &session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );
    let event_seen_after_conflict = cassie
        .midge
        .has_projection_event(&projection, "orders-stream", "event-1")
        .unwrap();
    let metadata_after_conflict = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();

    // Assert
    match failed {
        Err(CassieError::StorageRetryable(error)) => {
            assert_eq!(error.kind(), StorageRetryKind::WriteConflict);
        }
        Err(error) => panic!("unexpected replay error: {error}"),
        Ok(report) => panic!("conflicting metadata commit unexpectedly succeeded: {report:?}"),
    }
    assert_eq!(
        rows_after_conflict,
        vec![vec![Value::String("alpha".to_string())]]
    );
    assert!(!event_seen_after_conflict);
    assert_eq!(metadata_after_conflict.last_error.as_deref(), Some("concurrent schema writer"));
    assert_eq!(metadata_after_conflict.source_checkpoint, None);
    assert_eq!(metadata_after_conflict.last_applied_event_id, None);

    let retry = cassie.replay_projection_batch(batch).unwrap();
    let rows_after_retry = query_rows(
        &cassie,
        &session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );
    let metadata_after_retry = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let event_seen_after_retry = cassie
        .midge
        .has_projection_event(&projection, "orders-stream", "event-1")
        .unwrap();

    // Assert
    assert_eq!(retry.applied_event_count, 1);
    assert_eq!(rows_after_retry, rows_after_conflict);
    assert!(event_seen_after_retry);
    assert_eq!(
        metadata_after_retry.source_checkpoint.as_deref(),
        Some("checkpoint-1")
    );
    assert_eq!(
        metadata_after_retry.last_applied_event_id.as_deref(),
        Some("event-1")
    );

    cassie.shutdown();
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_projection_replay_checkpoint_after_uncertain_metadata_commit() {
    // Arrange
    let (_runtime, path, cassie, session, projection) =
        start_projection_replay_retry_fixture("projection_replay_uncertain_metadata_commit");
    cassie
        .replay_projection_batch(ProjectionReplayBatch {
            projection: projection.clone(),
            source_identity: "orders-stream".to_string(),
            batch_id: "batch-1".to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "event-1".to_string(),
                checkpoint: "checkpoint-1".to_string(),
                position: Some(1),
                document_id: "doc-1".to_string(),
                payload: Some(serde_json::json!({"title": "alpha"})),
            }],
        })
        .unwrap();
    let uncertain_batch = ProjectionReplayBatch {
        projection: projection.clone(),
        source_identity: "orders-stream".to_string(),
        batch_id: "batch-2".to_string(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-2".to_string(),
            checkpoint: "checkpoint-2".to_string(),
            position: Some(2),
            document_id: "doc-2".to_string(),
            payload: Some(serde_json::json!({"title": "bravo"})),
        }],
    };
    set_projection_replay_post_commit_failure_point(true);

    // Act
    let uncertain_commit = cassie.replay_projection_batch(uncertain_batch.clone());
    set_projection_replay_post_commit_failure_point(false);
    let metadata_after_uncertain_commit = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let event_seen_after_uncertain_commit = cassie
        .midge
        .has_projection_event(&projection, "orders-stream", "event-2")
        .unwrap();

    // Assert
    assert!(uncertain_commit.is_err());
    assert!(event_seen_after_uncertain_commit);
    assert_eq!(
        metadata_after_uncertain_commit.source_checkpoint.as_deref(),
        Some("checkpoint-2")
    );

    let retry = cassie.replay_projection_batch(uncertain_batch).unwrap();
    let metadata_after_retry = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let rows_after_retry = query_rows(
        &cassie,
        &session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );

    // Assert
    assert_eq!(retry.applied_event_count, 0);
    assert_eq!(retry.skipped_duplicate_count, 1);
    assert_eq!(
        metadata_after_retry.source_checkpoint.as_deref(),
        Some("checkpoint-2")
    );
    assert_eq!(
        rows_after_retry,
        vec![
            vec![Value::String("alpha".to_string())],
            vec![Value::String("bravo".to_string())],
        ]
    );

    cassie.shutdown();
    drop(cassie);
    let recovered = Cassie::new_with_data_dir(&path).unwrap();
    recovered.startup().unwrap();
    let recovered_session = recovered.create_session("tester", None);
    let recovered_rows = query_rows(
        &recovered,
        &recovered_session,
        "SELECT title FROM projection_replay_schema_retry_docs ORDER BY title",
    );
    let recovered_metadata = recovered
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let recovered_event_seen = recovered
        .midge
        .has_projection_event(&projection, "orders-stream", "event-2")
        .unwrap();

    // Assert
    assert_eq!(recovered_rows, rows_after_retry);
    assert!(recovered_event_seen);
    assert_eq!(
        recovered_metadata.source_checkpoint.as_deref(),
        Some("checkpoint-2")
    );

    recovered.shutdown();
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_hashed_projection_version_inactive_when_build_stops_before_metadata_publish() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_version_interrupted_hashes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_materialized_projection_version_fixture(&cassie, &session);
        set_projection_output_failure_point(Some(
            ProjectionOutputFailurePoint::AfterHashPublication,
        ));

        // Act
        let build = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
            vec![],
        );
        let metadata = cassie
            .catalog
            .get_materialized_projection("projection_versioned")
            .unwrap();
        let failed = metadata
            .versions
            .iter()
            .find(|version| version.version_id == "v2")
            .unwrap();
        let interrupted_output = failed.output_collection.clone();
        let visible_before_restart = query_rows(
            &cassie,
            &session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );
        cassie.shutdown();
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let visible_after_restart = query_rows(
            &restarted,
            &restarted_session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );

        // Assert
        assert!(build
            .expect_err("interrupted projection build")
            .to_string()
            .contains("after hash publication"));
        assert_eq!(metadata.active_version.as_deref(), Some("v1"));
        assert_eq!(failed.state, ProjectionVersionState::Failed);
        assert!(restarted
            .midge
            .root_hash(&interrupted_output)
            .expect("interrupted root hash")
            .is_some());
        assert!(!restarted
            .midge
            .list_range_hashes(&interrupted_output)
            .expect("interrupted range hashes")
            .is_empty());
        assert_eq!(
            visible_before_restart,
            vec![vec![Value::String("alpha".to_string())]]
        );
        assert_eq!(visible_after_restart, visible_before_restart);

        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_publish_projection_metadata_atomically_when_activation_commit_fails() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_version_metadata_commit_interrupted");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_materialized_projection_version_fixture(&cassie, &session);
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        set_projection_metadata_persistence_failure_point(true);

        // Act
        let activation = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
            vec![],
        );
        let metadata_before_restart = cassie
            .catalog
            .get_materialized_projection("projection_versioned")
            .unwrap();
        cassie.shutdown();
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let metadata_after_restart = restarted
            .catalog
            .get_materialized_projection("projection_versioned")
            .unwrap();
        let visible_after_restart = query_rows(
            &restarted,
            &restarted_session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );

        // Assert
        assert!(activation
            .expect_err("interrupted metadata commit")
            .to_string()
            .contains("projection metadata persistence"));
        assert_eq!(
            metadata_before_restart.active_version.as_deref(),
            Some("v1")
        );
        assert_eq!(metadata_after_restart.active_version.as_deref(), Some("v1"));
        assert_eq!(
            visible_after_restart,
            vec![vec![Value::String("alpha".to_string())]]
        );

        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    });
}
