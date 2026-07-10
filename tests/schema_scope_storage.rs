use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::types::Value;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-schema-scope-storage-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

#[test]
fn should_isolate_duplicate_relation_names_across_databases_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("restart_isolation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let postgres = cassie.create_session("tester", Some("postgres".to_string()));
            cassie
                .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
                .unwrap();
            cassie
                .execute_sql(&postgres, "CREATE SCHEMA reporting", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &postgres,
                    "CREATE TABLE reporting.docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &postgres,
                    "INSERT INTO reporting.docs (title) VALUES ('postgres-row')",
                    vec![],
                )
                .unwrap();

            let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
            cassie
                .execute_sql(&tenant, "CREATE SCHEMA reporting", vec![])
                .unwrap();
            cassie
                .execute_sql(&tenant, "CREATE TABLE reporting.docs (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &tenant,
                    "INSERT INTO reporting.docs (title) VALUES ('tenant-row')",
                    vec![],
                )
                .unwrap();

            cassie.shutdown();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let postgres = restarted.create_session("tester", Some("postgres".to_string()));
        let tenant = restarted.create_session("tester", Some("tenant_b".to_string()));

        // Act
        let postgres_rows = restarted
            .execute_sql(
                &postgres,
                "SELECT title FROM reporting.docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let tenant_rows = restarted
            .execute_sql(
                &tenant,
                "SELECT title FROM reporting.docs ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(postgres_rows.rows.len(), 1);
        assert_eq!(tenant_rows.rows.len(), 1);
        assert_eq!(
            postgres_rows.rows[0][0],
            Value::String("postgres-row".to_string())
        );
        assert_eq!(
            tenant_rows.rows[0][0],
            Value::String("tenant-row".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rewrite_collection_sidecars_when_schema_is_renamed() {
    // Arrange
    with_fallback();
    let path = data_dir("rename_schema_sidecars");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        let current = canonical_relation_name("postgres", "reporting", "metrics");
        let next = canonical_relation_name("postgres", "reporting_archive", "metrics");

        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE reporting.metrics (id INT PRIMARY KEY, status TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for id in 0..8 {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO reporting.metrics (id, status, body) VALUES ({id}, 'active', 'same')"
                    ),
                    vec![],
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_reporting_metrics_status ON reporting.metrics USING column (status, body) WITH (segment_size = 8)",
                vec![],
            )
            .unwrap();
        let current_index = cassie
            .catalog
            .list_indexes(&current)
            .into_iter()
            .find(|index| index.kind == cassie::catalog::IndexKind::Column)
            .map(|index| index.name)
            .expect("column index should be registered");
        assert!(cassie.midge.root_hash(&current).unwrap().is_some());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&current, &current_index)
            .unwrap()
            .is_some());

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER SCHEMA reporting RENAME TO reporting_archive",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id FROM reporting_archive.metrics ORDER BY id",
                vec![],
            )
            .unwrap();
        let next_index = cassie
            .catalog
            .list_indexes(&next)
            .into_iter()
            .find(|index| index.kind == cassie::catalog::IndexKind::Column)
            .map(|index| index.name)
            .expect("renamed column index should be registered");

        // Assert
        assert_eq!(rows.rows.len(), 8);
        assert!(cassie.midge.root_hash(&current).unwrap().is_none());
        assert!(cassie.midge.root_hash(&next).unwrap().is_some());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&current, &current_index)
            .unwrap()
            .is_none());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&next, &next_index)
            .unwrap()
            .is_some());
        assert!(cassie
            .execute_sql(
                &session,
                "SELECT id FROM reporting.metrics ORDER BY id",
                vec![],
            )
            .is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_dropping_non_empty_schema_without_cascade() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_non_empty_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE reporting.docs (title TEXT)", vec![])
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(&session, "DROP SCHEMA reporting", vec![])
            .expect_err("non-empty schema should be rejected");

        // Assert
        assert!(error
            .to_string()
            .contains("namespace 'postgres.reporting' is not empty"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_dropping_current_or_non_empty_database() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_database_guards");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let postgres = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
            .unwrap();
        let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
        cassie
            .execute_sql(&tenant, "CREATE TABLE public.docs (title TEXT)", vec![])
            .unwrap();

        // Act
        let current_error = cassie
            .execute_sql(&tenant, "DROP DATABASE tenant_b", vec![])
            .expect_err("current database should be protected");
        let non_empty_error = cassie
            .execute_sql(&postgres, "DROP DATABASE tenant_b", vec![])
            .expect_err("non-empty database should be rejected");

        // Assert
        assert!(current_error
            .to_string()
            .contains("cannot drop the currently open database 'tenant_b'"));
        assert!(non_empty_error
            .to_string()
            .contains("database 'tenant_b' is not empty"));

        let _ = std::fs::remove_dir_all(path);
    });
}
