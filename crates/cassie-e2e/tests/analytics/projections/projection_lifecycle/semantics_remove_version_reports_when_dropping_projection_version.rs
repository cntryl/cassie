#[test]
fn should_remove_version_reports_when_dropping_projection_version() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_version_report_cleanup");
    runtime.block_on(async {
            execute_statement(
                &cassie,
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
            );
            for version in ["v1", "v2"] {
                execute_statement(
                    &cassie,
                    &session,
                    &format!(
                        "COMPARE PROJECTION projection_versioned VERSION {version} WITH MANIFEST '{{\"root_digest\":\"abc\"}}'"
                    ),
                );
                let report = cassie::catalog::ProjectionRepairReportMeta {
                    report_id: format!("repair-report-{version}"),
                    created_ms: 1,
                    projection_name: projection.clone(),
                    target: projection.clone(),
                    version_id: Some(version.to_string()),
                    scope: "projection-version".to_string(),
                    action: "rebuild".to_string(),
                    state: "completed".to_string(),
                    executable: true,
                    affected_objects: Vec::new(),
                    source_report_state: "failed".to_string(),
                    source_mismatch_count: 0,
                    source_missing_count: 0,
                    source_stale_count: 0,
                    verification_required: "full".to_string(),
                    post_verification_state: "verified".to_string(),
                    last_error: None,
                };
                cassie.midge.put_projection_repair_report(&report).unwrap();
                cassie.catalog.register_projection_repair_report(report);
            }

            // Act
            execute_statement(
                &cassie,
                &session,
                "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v2",
            );
            let comparison_versions = cassie
                .catalog
                .list_projection_comparison_reports()
                .into_iter()
                .map(|report| report.target_version_id)
                .collect::<Vec<_>>();
            let repair_versions = cassie
                .catalog
                .list_projection_repair_reports()
                .into_iter()
                .map(|report| report.version_id)
                .collect::<Vec<_>>();
            let stored_repair_versions = cassie
                .midge
                .list_projection_repair_reports()
                .unwrap()
                .into_iter()
                .map(|report| report.version_id)
                .chain(
                    cassie
                        .midge
                        .list_projection_comparison_reports()
                        .unwrap()
                        .into_iter()
                        .map(|report| report.target_version_id),
                )
                .collect::<Vec<_>>();

            // Assert
            let v1 = Some("v1".to_string());
            assert_eq!(comparison_versions, vec![v1.clone()]);
            assert_eq!(repair_versions, vec![v1.clone()]);
            assert_eq!(stored_repair_versions, vec![v1.clone(), v1]);
        });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_drop_quoted_mixed_case_materialized_projection_referenced_unquoted() {
    // Arrange
    let (runtime, path, cassie, session, _projection) =
        start_projection_version_fixture("projection_mixed_case_drop");
    runtime.block_on(async {
            execute_statement(
                &cassie,
                &session,
                "CREATE MATERIALIZED PROJECTION \"MixedCaseProjection\" AS SELECT title FROM projection_version_docs",
            );

            // Act
            let dropped = cassie.execute_sql(
                &session,
                "DROP MATERIALIZED PROJECTION mixedcaseprojection",
                vec![],
            );
            let remaining = cassie
                .catalog
                .list_projection_metadata()
                .into_iter()
                .map(|projection| projection.collection)
                .filter(|collection| collection.to_ascii_lowercase().contains("mixedcase"))
                .collect::<Vec<_>>();

            // Assert
            assert!(
                dropped.is_ok(),
                "DROP MATERIALIZED PROJECTION failed: {dropped:?}"
            );
            assert!(
                remaining.is_empty(),
                "projection metadata remained after the drop: {remaining:?}"
            );
        });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_projection_version_intact_when_report_deletion_fails() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_version_report_deletion_failure");
    runtime.block_on(async {
            execute_statement(
                &cassie,
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
            );
            execute_statement(
                &cassie,
                &session,
                "COMPARE PROJECTION projection_versioned VERSION v2 WITH MANIFEST '{\"root_digest\":\"abc\"}'",
            );
            set_projection_report_deletion_failure_point(true);

            // Act
            let dropped = cassie.execute_sql(
                &session,
                "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v2",
                vec![],
            );
            set_projection_report_deletion_failure_point(false);
            let versions = projection_version_rows(&cassie, &session, &projection);
            let stored_report_versions = cassie
                .midge
                .list_projection_comparison_reports()
                .unwrap()
                .into_iter()
                .map(|report| report.target_version_id)
                .collect::<Vec<_>>();
            let catalog_report_versions = cassie
                .catalog
                .list_projection_comparison_reports()
                .into_iter()
                .map(|report| report.target_version_id)
                .collect::<Vec<_>>();
            let version_output = cassie
                .catalog
                .get_materialized_projection("projection_versioned")
                .expect("projection metadata")
                .versions
                .into_iter()
                .find(|version| version.version_id == "v2")
                .map(|version| version.output_collection);

            // Assert
            assert!(
                dropped.is_err(),
                "drop succeeded although report deletion failed: {dropped:?}"
            );
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
            assert_eq!(stored_report_versions, vec![Some("v2".to_string())]);
            assert_eq!(catalog_report_versions, vec![Some("v2".to_string())]);
            let output = version_output.expect("v2 output collection");
            assert!(
                cassie.midge.collection_schema(&output).is_some(),
                "version output collection was dropped: {output}"
            );
        });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_active_projection_visible_when_version_build_stops_after_row_batches() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_version_interrupted_rows");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_materialized_projection_version_fixture(&cassie, &session);
        set_projection_output_failure_point(Some(ProjectionOutputFailurePoint::AfterRowBatches));

        // Act
        let build = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
            vec![],
        );
        let visible_before_restart = query_rows(
            &cassie,
            &session,
            "SELECT title FROM projection_versioned ORDER BY title",
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
            .contains("after row batches"));
        assert_eq!(metadata.active_version.as_deref(), Some("v1"));
        assert_eq!(failed.state, ProjectionVersionState::Failed);
        assert!(restarted
            .midge
            .root_hash(&interrupted_output)
            .expect("interrupted root hash")
            .is_none());
        assert!(restarted
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
fn should_keep_previous_projection_active_when_activation_stops_before_publish() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_version_interrupted_activation");
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
        set_projection_activation_failure_point(true);

        // Act
        let activation = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
            vec![],
        );
        let metadata = cassie
            .catalog
            .get_materialized_projection("projection_versioned")
            .unwrap();
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
        execute_statement(
            &restarted,
            &restarted_session,
            "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
        );
        let visible_after_retry = query_rows(
            &restarted,
            &restarted_session,
            "SELECT title FROM projection_versioned ORDER BY title",
        );

        // Assert
        assert!(activation
            .expect_err("interrupted projection activation")
            .to_string()
            .contains("before activation publication"));
        assert_eq!(metadata.active_version.as_deref(), Some("v1"));
        assert_eq!(
            metadata
                .versions
                .iter()
                .find(|version| version.version_id == "v2")
                .unwrap()
                .state,
            ProjectionVersionState::Built
        );
        assert_eq!(
            visible_before_restart,
            vec![vec![Value::String("alpha".to_string())]]
        );
        assert_eq!(visible_after_restart, visible_before_restart);
        assert_eq!(
            visible_after_retry,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("bravo".to_string())],
            ]
        );

        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    });
}
