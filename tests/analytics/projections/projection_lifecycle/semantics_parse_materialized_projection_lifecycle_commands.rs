#[test]
fn should_parse_materialized_projection_lifecycle_commands() {
    // Arrange
    let create_sql = "CREATE MATERIALIZED PROJECTION projection_ready AS SELECT title FROM docs";
    let activate_sql = "ALTER MATERIALIZED PROJECTION projection_ready ACTIVATE VERSION v2 UNSAFE";
    let drop_version_sql = "DROP MATERIALIZED PROJECTION VERSION projection_ready VERSION v1";

    // Act
    let create = cassie::sql::parse_statement(create_sql).unwrap();
    let activate = cassie::sql::parse_statement(activate_sql).unwrap();
    let drop_version = cassie::sql::parse_statement(drop_version_sql).unwrap();

    // Assert
    let QueryStatement::CreateMaterializedProjection(create) = create.statement else {
        panic!("expected CREATE MATERIALIZED PROJECTION");
    };
    assert_eq!(create.name, "projection_ready");
    assert_eq!(create.query, "SELECT title FROM docs");

    let QueryStatement::AlterMaterializedProjection(activate) = activate.statement else {
        panic!("expected ALTER MATERIALIZED PROJECTION");
    };
    match activate.operation {
        AlterMaterializedProjectionOperation::ActivateVersion {
            version_id,
            unsafe_override,
        } => {
            assert_eq!(version_id, "v2");
            assert!(unsafe_override);
        }
        AlterMaterializedProjectionOperation::BuildVersion => {
            panic!("expected activate version")
        }
    }

    let QueryStatement::DropMaterializedProjectionVersion(drop_version) = drop_version.statement
    else {
        panic!("expected DROP MATERIALIZED PROJECTION VERSION");
    };
    assert_eq!(drop_version.name, "projection_ready");
    assert_eq!(drop_version.version_id, "v1");
}

#[test]
fn should_skip_existing_projection_events_while_applying_new_replay_events() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_replay_mixed_duplicate_batch");
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
                "CREATE TABLE projection_replay_mixed_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_mixed_docs");
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

        // Act
        let replay = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection,
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-2".to_string(),
                lag: 0,
                events: vec![
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-1".to_string(),
                        position: Some(1),
                        document_id: "doc-1".to_string(),
                        payload: Some(serde_json::json!({"title": "alpha-updated"})),
                    },
                    ProjectionReplayEvent {
                        event_id: "event-2".to_string(),
                        checkpoint: "checkpoint-2".to_string(),
                        position: Some(2),
                        document_id: "doc-2".to_string(),
                        payload: Some(serde_json::json!({"title": "bravo"})),
                    },
                ],
            })
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_mixed_docs ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(replay.applied_event_count, 1);
        assert_eq!(replay.skipped_duplicate_count, 1);
        assert_eq!(
            selected.rows(),
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("bravo".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_mark_large_replay_hashes_stale_without_eager_rebuild() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_replay_large_hashes_stale");
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
                "CREATE TABLE projection_replay_large_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_large_docs");
        let mut csv = String::new();
        for index in 0..600 {
            use std::fmt::Write as _;
            writeln!(csv, "bulk-{index},title-{index},{index}").unwrap();
        }
        cassie
            .copy_from_csv_stdin(
                &session,
                &CopyStatement {
                    table: projection.clone(),
                    columns: vec!["_id".to_string(), "title".to_string(), "score".to_string()],
                    format: CopyFormat::Csv,
                    header: false,
                },
                csv.as_bytes(),
            )
            .unwrap();

        // Act
        cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection,
                source_identity: "large-replay-stream".to_string(),
                batch_id: "large-replay-batch".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "large-replay-event".to_string(),
                    checkpoint: "large-checkpoint".to_string(),
                    position: Some(1),
                    document_id: "large-replay-doc".to_string(),
                    payload: Some(serde_json::json!({
                        "title": "replayed",
                        "score": 601,
                    })),
                }],
            })
            .unwrap();
        let metadata = cassie
            .catalog
            .get_projection_metadata("projection_replay_large_docs")
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_large_docs WHERE score = 601",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("replayed".to_string())]]
        );
        assert_eq!(
            metadata.hashes.root.state,
            ProjectionVerificationState::Stale
        );
        assert_eq!(metadata.hashes.root.row_count, 601);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_failed_freshness_for_out_of_order_replay() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_replay_out_of_order");
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
                "CREATE TABLE projection_replay_order_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_order_docs");
        cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-1".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-2".to_string(),
                    checkpoint: "checkpoint-2".to_string(),
                    position: Some(2),
                    document_id: "doc-2".to_string(),
                    payload: Some(serde_json::json!({"title": "bravo"})),
                }],
            })
            .unwrap();

        // Act
        let error = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-2".to_string(),
                lag: 1,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-1".to_string(),
                    checkpoint: "checkpoint-1".to_string(),
                    position: Some(1),
                    document_id: "doc-1".to_string(),
                    payload: Some(serde_json::json!({"title": "alpha"})),
                }],
            })
            .unwrap_err();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert!(error.to_string().contains("out-of-order"));
        assert_eq!(checkpoint.rows().len(), 1);
        assert_eq!(checkpoint.rows()[0][0], Value::String("failed".to_string()));
        assert!(matches!(&checkpoint.rows()[0][1], Value::String(message) if message.contains("out-of-order")));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fail_duplicate_event_id_in_batch_without_partial_replay() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_replay_duplicate_in_batch");
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
                "CREATE TABLE projection_replay_conflict_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_conflict_docs");

        // Act
        let error = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-conflict".to_string(),
                lag: 2,
                events: vec![
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-1".to_string(),
                        position: Some(1),
                        document_id: "doc-1".to_string(),
                        payload: Some(serde_json::json!({"title": "alpha"})),
                    },
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-2".to_string(),
                        position: Some(2),
                        document_id: "doc-2".to_string(),
                        payload: Some(serde_json::json!({"title": "bravo"})),
                    },
                ],
            })
            .unwrap_err();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_conflict_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, replay_batch_id, source_checkpoint, last_applied_event_id, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let restarted_checkpoint = restarted
            .execute_sql(
                &restarted_session,
                &format!(
                    "SELECT freshness, replay_batch_id, source_checkpoint, last_applied_event_id, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert!(error.to_string().contains("duplicate projection replay event"));
        assert_eq!(selected.rows(), Vec::<Vec<Value>>::new());
        assert_eq!(checkpoint.rows().len(), 1);
        assert_eq!(checkpoint.rows()[0][0], Value::String("failed".to_string()));
        assert_eq!(
            checkpoint.rows()[0][1],
            Value::String("batch-conflict".to_string())
        );
        assert_eq!(checkpoint.rows()[0][2], Value::String(String::new()));
        assert_eq!(checkpoint.rows()[0][3], Value::String(String::new()));
        assert!(matches!(&checkpoint.rows()[0][4], Value::String(message) if message.contains("duplicate projection replay event")));
        assert_eq!(restarted_checkpoint.rows(), checkpoint.rows());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_refresh_materialized_projection_after_source_write() {
    // Arrange
    use_local_storage();
    let path = data_dir("materialized_projection_refresh");
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
                "CREATE TABLE projection_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_ready AS SELECT title, score FROM projection_source_docs WHERE score > 1",
                vec![],
            )
            .unwrap();
        let projection = cassie
            .catalog
            .get_materialized_projection("projection_ready")
            .expect("materialized projection metadata")
            .collection;
        let initial = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();
        let stale = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state FROM pg_catalog.pg_materialized_projections WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_ready",
                vec![],
            )
            .unwrap();
        let refreshed = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            initial.rows(),
            vec![vec![Value::String("bravo".to_string()), Value::Int64(2)]]
        );
        assert_eq!(stale.rows(), vec![vec![Value::String("stale".to_string())]]);
        assert_eq!(
            refreshed.rows(),
            vec![
                vec![Value::String("bravo".to_string())],
                vec![Value::String("charlie".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_activate_built_materialized_projection_version() {
    // Arrange
    use_local_storage();
    let path = data_dir("materialized_projection_versions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let projection = seed_materialized_projection_version_fixture(&cassie, &session);

        // Act
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        let before_swap = query_rows(
            &cassie,
            &session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
        );
        let after_swap = query_rows(
            &cassie,
            &session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );
        execute_statement(
            &cassie,
            &session,
            "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v1",
        );
        let versions = projection_version_rows(&cassie, &session, &projection);

        // Assert
        assert_eq!(before_swap, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(
            after_swap,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("bravo".to_string())],
            ]
        );
        assert_eq!(
            versions,
            vec![vec![
                Value::String("v2".to_string()),
                Value::String("active".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_allocate_new_projection_version_id_after_dropping_older_version() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_version_id_after_drop");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let projection = seed_materialized_projection_version_fixture(&cassie, &session);
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
        );
        execute_statement(
            &cassie,
            &session,
            "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v1",
        );
        let served_before_build = query_rows(
            &cassie,
            &session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );
        execute_statement(
            &cassie,
            &session,
            "INSERT INTO projection_version_docs (title) VALUES ('charlie')",
        );

        // Act
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        let served_after_build = query_rows(
            &cassie,
            &session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );
        let versions = projection_version_rows(&cassie, &session, &projection);
        let metadata = cassie
            .catalog
            .get_materialized_projection("projection_versioned")
            .expect("materialized projection metadata");

        // Assert
        assert_eq!(
            versions,
            vec![
                vec![
                    Value::String("v2".to_string()),
                    Value::String("active".to_string())
                ],
                vec![
                    Value::String("v3".to_string()),
                    Value::String("built".to_string())
                ],
            ]
        );
        assert_eq!(metadata.active_version.as_deref(), Some("v2"));
        assert_eq!(served_after_build, served_before_build);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_reuse_projection_version_id_after_dropping_highest_version() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_version_id_after_highest_drop");
    runtime.block_on(async {
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        execute_statement(
            &cassie,
            &session,
            "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v2",
        );

        // Act
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        let versions = projection_version_rows(&cassie, &session, &projection);

        // Assert
        assert_eq!(
            versions,
            vec![
                vec![
                    Value::String("v1".to_string()),
                    Value::String("active".to_string())
                ],
                vec![
                    Value::String("v3".to_string()),
                    Value::String("built".to_string())
                ],
            ]
        );
    });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_build_projection_version_after_dropping_conflicting_output_collection() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_version_output_recovery");
    runtime.block_on(async {
        execute_statement(
            &cassie,
            &session,
            "CREATE TABLE __cassie_projection_projection_versioned_v2 (title TEXT)",
        );
        let _ = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
            vec![],
        );
        execute_statement(
            &cassie,
            &session,
            "DROP TABLE __cassie_projection_projection_versioned_v2",
        );

        // Act
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        let versions = projection_version_rows(&cassie, &session, &projection);

        // Assert
        assert_eq!(
            versions,
            vec![
                vec![
                    Value::String("v1".to_string()),
                    Value::String("active".to_string())
                ],
                vec![
                    Value::String("v2".to_string()),
                    Value::String("built".to_string())
                ],
            ]
        );
    });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
