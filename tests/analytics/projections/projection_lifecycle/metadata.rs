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
