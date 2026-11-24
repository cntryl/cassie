#[test]
fn should_reject_dml_against_materialized_projection_output() {
    // Arrange
    let path = data_dir("materialized_projection_read_only");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE projection_ro_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_ro_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_ro AS SELECT title FROM projection_ro_docs",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_ro (title) VALUES ('bravo')",
                vec![],
            )
            .unwrap_err();

        // Assert
        assert!(error.to_string().contains("read-only"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_projection_version_build_when_output_collection_already_exists() {
    // Arrange
    let (runtime, path, cassie, session, projection) =
        start_projection_version_fixture("projection_version_output_exists");
    runtime.block_on(async {
        execute_statement(
            &cassie,
            &session,
            "CREATE TABLE __cassie_projection_projection_versioned_v2 (title TEXT)",
        );

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
                vec![],
            )
            .expect_err("build over an existing output collection should fail");
        let versions = projection_version_rows(&cassie, &session, &projection);

        // Assert
        assert!(
            error
                .to_string()
                .contains("__cassie_projection_projection_versioned_v2"),
            "error should name the existing output collection: {error}"
        );
        assert_eq!(
            versions,
            vec![vec![
                Value::String("v1".to_string()),
                Value::String("active".to_string())
            ]]
        );
    });
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
