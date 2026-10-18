#[test]
fn should_keep_projection_version_output_when_metadata_persistence_fails() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_version_persist_failure");
    runtime.block_on(async {
        execute_statement(
            &cassie,
            &session,
            "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
        );
        set_projection_metadata_persistence_failure_point(true);

        // Act
        let dropped = cassie.execute_sql(
            &session,
            "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v2",
            vec![],
        );
        set_projection_metadata_persistence_failure_point(false);
        let versions = projection_version_rows(&cassie, &session, &projection);
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
            "drop succeeded although metadata persistence failed: {dropped:?}"
        );
        assert_eq!(versions.len(), 2, "version metadata changed: {versions:?}");
        let output = version_output.expect("v2 output collection");
        assert!(
            cassie.midge.collection_schema(&output).is_some(),
            "version output collection was dropped before its metadata: {output}"
        );
    });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_delete_projection_reports_when_dropping_a_projection_before_restart() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_drop_report_cleanup");
    runtime.block_on(async {
            execute_statement(
                &cassie,
                &session,
                "COMPARE PROJECTION projection_versioned VERSION v1 WITH MANIFEST '{\"root_digest\":\"abc\"}'",
            );
            let report = cassie::catalog::ProjectionRepairReportMeta {
                report_id: "repair-report-dropped".to_string(),
                created_ms: 1,
                projection_name: projection.clone(),
                target: projection.clone(),
                version_id: Some("v1".to_string()),
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

            // Act
            execute_statement(
                &cassie,
                &session,
                "DROP MATERIALIZED PROJECTION projection_versioned",
            );
        });
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let repair_reports = restarted.catalog.list_projection_repair_reports().len();
    let comparison_reports = restarted.catalog.list_projection_comparison_reports().len();

    // Assert
    assert_eq!(
        (repair_reports, comparison_reports),
        (0, 0),
        "reports of a dropped projection reloaded after restart"
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_hydrate_materialized_projection_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("materialized_projection_restart");
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
                "CREATE TABLE projection_restart_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_restart_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_restart AS SELECT title FROM projection_restart_docs",
                vec![],
            )
            .unwrap();
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT title FROM projection_restart ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
