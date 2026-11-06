#[test]
fn should_preserve_declared_type_identity_with_typmod_after_restart() {
    // Arrange
    super::support_sql::use_local_storage();
    let path = super::support_sql::data_dir("issue-904-type-catalog-restart");
    let _directory = TypeCatalogDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("first Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_904_persisted_type (amount NUMERIC(5, 2))",
                vec![],
            )
            .expect("persisted numeric column");
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopened Cassie instance");
    cassie.startup().expect("restart startup");
    let session = cassie.create_session("tester", None);
    let selected = cassie
        .execute_sql(
            &session,
            "SELECT atttypid, atttypmod FROM pg_attribute WHERE attrelid = 'issue_904_persisted_type' AND attname = 'amount'",
            vec![],
        )
        .expect("persisted type descriptor");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(1_700), Value::Int64(327_686)]]
    );
}

#[test]
fn should_preserve_unpopulated_materialized_view_vector_type_after_restart() {
    // Arrange
    const CHILD: &str = "CASSIE_TEST_MATVIEW_VECTOR_TYPE_RESTART_CHILD";
    const COMPLETE: &str = "matview-vector-type-restart-complete";
    if std::env::var_os(CHILD).is_none() {
        let mut command =
            std::process::Command::new(std::env::current_exe().expect("test executable"));
        command.args([
            "--exact",
            "type_catalog_discovery::should_preserve_unpopulated_materialized_view_vector_type_after_restart",
            "--nocapture",
            "--test-threads=1",
        ]);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("CASSIE_STORAGE_") {
                command.env_remove(key);
            }
        }
        let output = command
            .env(CHILD, "1")
            .env("CASSIE_STORAGE_MODE", "local")
            .output()
            .expect("local-storage child");
        assert!(
            output.status.success(),
            "local-storage child failed: {}",
            output.status
        );
        assert!(String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line == COMPLETE));
        return;
    }
    let path = super::support_sql::data_dir("matview-vector-type-restart");
    let _directory = TypeCatalogDirectory(path.clone());
    let config =
        super::support_sql::with_test_root_password(cassie::config::CassieRuntimeConfig::default());
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("initial Cassie");
        cassie.startup().expect("initial startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE mv_restart_source (id INTEGER)",
            "CREATE MATERIALIZED VIEW mv_restart_projection AS SELECT CAST(NULL AS VECTOR(19)) AS embedding FROM mv_restart_source WITH NO DATA",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect("persist fixture metadata");
        }
        drop(session);
        cassie.shutdown();
    }

    // Act
    let reopened = Cassie::new_with_data_dir_and_config(&path, config).expect("reopened Cassie");
    reopened.startup().expect("restart startup");
    let session = reopened.create_session("tester", None);
    let selected = reopened.execute_sql(&session, "SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type WHERE typname = 'vector(19)'", vec![]).expect("recovered vector identity");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![type_row(100_019, "vector(19)", -1, 0)]
    );
    drop(session);
    reopened.shutdown();
    println!("{COMPLETE}");
}
