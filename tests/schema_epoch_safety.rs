use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-schema-epoch-{name}-{}", Uuid::new_v4()))
}

fn compile_physical_plan(
    cassie: &Cassie,
    sql: &str,
) -> Arc<cassie::planner::physical::PhysicalPlan> {
    cassie
        .compile_sql_physical_plan_for_diagnostics(sql)
        .unwrap()
}

fn scalar_index_sidecars(cassie: &Cassie, collection: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let prefix =
        cassie::midge::adapter::Midge::scalar_index_collection_prefix_for_diagnostics(collection);
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, prefix.as_slice())
        .unwrap()
}

#[test]
fn should_defer_drop_table_physical_cleanup_until_pinned_schema_epoch_drains() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_table_deferred_cleanup");
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
                "CREATE TABLE epoch_drop_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO epoch_drop_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("epoch_drop_docs")
            .expect("catalog collection")
            .collection;
        let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();

        // Act
        cassie
            .execute_sql(&session, "DROP TABLE epoch_drop_docs", vec![])
            .unwrap();
        let new_query = cassie.execute_sql(&session, "SELECT title FROM epoch_drop_docs", vec![]);
        let rows_while_pinned = cassie.midge.scan_documents(&collection).unwrap();
        cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .unwrap();
        let rows_after_pinned_cleanup = cassie.midge.scan_documents(&collection).unwrap();
        drop(pinned);
        cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .unwrap();
        let rows_after_drain = cassie.midge.scan_documents(&collection);

        // Assert
        assert!(new_query.is_err());
        assert_eq!(rows_while_pinned.len(), 1);
        assert_eq!(
            rows_while_pinned[0].payload["title"],
            serde_json::json!("alpha")
        );
        assert_eq!(rows_after_pinned_cleanup.len(), 1);
        assert!(rows_after_drain.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_defer_drop_index_sidecar_cleanup_until_pinned_schema_epoch_drains() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_index_deferred_cleanup");
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
                "CREATE TABLE epoch_drop_index_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO epoch_drop_index_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX epoch_drop_title_idx ON epoch_drop_index_docs USING btree (title)",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("epoch_drop_index_docs")
            .expect("catalog collection")
            .collection;
        let stored_index_name = cassie
            .catalog
            .get_index(&collection, "epoch_drop_title_idx")
            .expect("catalog index")
            .name;
        let sidecars_before = scalar_index_sidecars(&cassie, &collection);
        let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();

        // Act
        cassie
            .execute_sql(
                &session,
                "DROP INDEX epoch_drop_title_idx ON epoch_drop_index_docs",
                vec![],
            )
            .unwrap();
        let new_plan = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM epoch_drop_index_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .unwrap();
        let sidecars_while_pinned = scalar_index_sidecars(&cassie, &collection);
        let stored_index_while_pinned = cassie
            .midge
            .get_index(&collection, &stored_index_name)
            .unwrap();
        drop(pinned);
        cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .unwrap();
        let sidecars_after_drain = scalar_index_sidecars(&cassie, &collection);
        let stored_index_after_drain = cassie
            .midge
            .get_index(&collection, &stored_index_name)
            .unwrap();

        // Assert
        assert!(!sidecars_before.is_empty());
        let cassie::types::Value::String(plan) = &new_plan.rows[0][0] else {
            panic!("expected explain text");
        };
        assert!(plan.contains("index=none"), "plan={plan}");
        assert!(cassie
            .catalog
            .get_index(&collection, "epoch_drop_title_idx")
            .is_none());
        assert!(!sidecars_while_pinned.is_empty());
        assert!(stored_index_while_pinned.is_some());
        assert!(sidecars_after_drain.is_empty());
        assert!(stored_index_after_drain.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_defer_drop_view_metadata_cleanup_until_pinned_schema_epoch_drains() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_view_deferred_cleanup");
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
                "CREATE TABLE epoch_view_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW epoch_view_ready AS SELECT title FROM epoch_view_docs",
                vec![],
            )
            .unwrap();
        let view = cassie
            .catalog
            .get_view("epoch_view_ready")
            .expect("catalog view")
            .name;
        let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();

        // Act
        cassie
            .execute_sql(&session, "DROP VIEW epoch_view_ready", vec![])
            .unwrap();
        let new_query = cassie.execute_sql(&session, "SELECT title FROM epoch_view_ready", vec![]);
        let view_while_pinned = cassie.midge.get_view(&view).unwrap();
        cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .unwrap();
        let view_after_pinned_cleanup = cassie.midge.get_view(&view).unwrap();
        drop(pinned);
        cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .unwrap();
        let view_after_drain = cassie.midge.get_view(&view).unwrap();

        // Assert
        assert!(new_query.is_err());
        assert!(view_while_pinned.is_some());
        assert!(view_after_pinned_cleanup.is_some());
        assert!(view_after_drain.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_finish_pending_schema_cleanup_on_startup_without_rehydrating_dropped_table() {
    // Arrange
    with_fallback();
    let path = data_dir("startup_pending_cleanup");
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
                "CREATE TABLE epoch_restart_drop_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO epoch_restart_drop_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("epoch_restart_drop_docs")
            .expect("catalog collection")
            .collection;
        let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();
        cassie
            .execute_sql(&session, "DROP TABLE epoch_restart_drop_docs", vec![])
            .unwrap();
        let rows_before_restart = cassie.midge.scan_documents(&collection).unwrap();
        drop(pinned);
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let new_query = restarted.execute_sql(
            &session,
            "SELECT title FROM epoch_restart_drop_docs",
            vec![],
        );
        let physical_rows = restarted.midge.scan_documents(&collection);

        // Assert
        assert_eq!(rows_before_restart.len(), 1);
        assert!(new_query.is_err());
        assert!(physical_rows.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_saved_plan_with_schema_snapshot_after_column_rename() {
    // Arrange
    with_fallback();
    let path = data_dir("saved_plan_column_rename");
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
                "CREATE TABLE epoch_rename_docs (id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO epoch_rename_docs (id, title) VALUES ('d1', 'alpha')",
                vec![],
            )
            .unwrap();
        let saved_plan = compile_physical_plan(
            &cassie,
            "SELECT id, title FROM epoch_rename_docs ORDER BY id",
        );

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE epoch_rename_docs RENAME COLUMN title TO headline",
                vec![],
            )
            .unwrap();
        let saved = cassie
            .execute_physical_plan_for_diagnostics(&session, &saved_plan)
            .unwrap();
        let current = cassie
            .execute_sql(
                &session,
                "SELECT id, headline FROM epoch_rename_docs ORDER BY id",
                vec![],
            )
            .unwrap();
        let old_name = cassie.execute_sql(
            &session,
            "SELECT id, title FROM epoch_rename_docs ORDER BY id",
            vec![],
        );

        // Assert
        assert_eq!(saved.columns[1].name, "title");
        assert_eq!(
            saved.rows[0][1],
            cassie::types::Value::String("alpha".to_string())
        );
        assert_eq!(current.columns[1].name, "headline");
        assert_eq!(
            current.rows[0][1],
            cassie::types::Value::String("alpha".to_string())
        );
        assert!(old_name.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_saved_plan_with_schema_snapshot_after_column_drop() {
    // Arrange
    with_fallback();
    let path = data_dir("saved_plan_column_drop");
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
                "CREATE TABLE epoch_drop_column_docs (id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO epoch_drop_column_docs (id, title) VALUES ('d1', 'alpha')",
                vec![],
            )
            .unwrap();
        let saved_plan = compile_physical_plan(
            &cassie,
            "SELECT id, title FROM epoch_drop_column_docs ORDER BY id",
        );

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE epoch_drop_column_docs DROP COLUMN title",
                vec![],
            )
            .unwrap();
        let saved = cassie
            .execute_physical_plan_for_diagnostics(&session, &saved_plan)
            .unwrap();
        let current = cassie
            .execute_sql(
                &session,
                "SELECT id FROM epoch_drop_column_docs ORDER BY id",
                vec![],
            )
            .unwrap();
        let dropped = cassie.execute_sql(
            &session,
            "SELECT title FROM epoch_drop_column_docs ORDER BY id",
            vec![],
        );

        // Assert
        assert_eq!(saved.columns[1].name, "title");
        assert_eq!(
            saved.rows[0][1],
            cassie::types::Value::String("alpha".to_string())
        );
        assert_eq!(current.columns.len(), 1);
        assert!(dropped.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_saved_wildcard_plan_on_schema_snapshot_after_column_add() {
    // Arrange
    with_fallback();
    let path = data_dir("saved_plan_column_add");
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
                "CREATE TABLE epoch_add_column_docs (id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO epoch_add_column_docs (id, title) VALUES ('d1', 'alpha')",
                vec![],
            )
            .unwrap();
        let saved_plan =
            compile_physical_plan(&cassie, "SELECT * FROM epoch_add_column_docs ORDER BY id");

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE epoch_add_column_docs ADD COLUMN status TEXT",
                vec![],
            )
            .unwrap();
        let saved = cassie
            .execute_physical_plan_for_diagnostics(&session, &saved_plan)
            .unwrap();
        let current = cassie
            .execute_sql(
                &session,
                "SELECT * FROM epoch_add_column_docs ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            saved
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "title"]
        );
        assert_eq!(
            current
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "title", "status"]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
