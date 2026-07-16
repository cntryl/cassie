use cassie::app::Cassie;
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-upsert-{name}-{}", Uuid::new_v4()))
}

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

#[test]
fn should_update_conflicting_row_given_parameters_excluded_filter_and_returning() {
    // Arrange
    with_fallback();
    let path = data_dir("update");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "CREATE TABLE upsert_docs (id INT PRIMARY KEY, tenant TEXT, title TEXT, note TEXT)", vec![]).unwrap();
        cassie.execute_sql(&session, "CREATE UNIQUE INDEX upsert_tenant_title ON upsert_docs (tenant, title)", vec![]).unwrap();
        cassie.execute_sql(&session, "INSERT INTO upsert_docs (id, tenant, title, note) VALUES (1, 'a', 'one', 'keep')", vec![]).unwrap();

        // Act
        let result = cassie.execute_sql(
            &session,
            "INSERT INTO upsert_docs (id, tenant, title) VALUES ($1, $2, $3) ON CONFLICT (tenant, title) DO UPDATE SET title = excluded.title WHERE upsert_docs.title = excluded.title RETURNING title, note",
            vec![Value::Int64(1), Value::String("a".into()), Value::String("one".into())],
        ).unwrap();

        // Assert
        assert_eq!(result.command, "INSERT 0 1");
        assert_eq!(result.rows, vec![vec![Value::String("one".into()), Value::String("keep".into())]]);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_invalid_conflict_update_before_mutation() {
    // Arrange
    with_fallback();
    let path = data_dir("binding");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "CREATE TABLE upsert_bind (id INT PRIMARY KEY, title TEXT)", vec![]).unwrap();
        cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (1, 'alpha')", vec![]).unwrap();

        // Act
        let unknown = cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.missing", vec![]);
        let duplicate = cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title, title = 'again'", vec![]);
        let non_unique = cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (2, 'alpha') ON CONFLICT (title) DO UPDATE SET title = excluded.title", vec![]);
        let rows = cassie.execute_sql(&session, "SELECT title FROM upsert_bind", vec![]).unwrap();

        // Assert
        assert!(unknown.unwrap_err().to_string().contains("excluded.missing"));
        assert!(duplicate.unwrap_err().to_string().contains("duplicated"));
        assert!(non_unique.unwrap_err().to_string().contains("does not match"));
        assert_eq!(rows.rows, vec![vec![Value::String("alpha".into())]]);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_only_committed_upsert_given_rollback_then_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transactions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "CREATE TABLE upsert_tx (id INT PRIMARY KEY, title TEXT)", vec![]).unwrap();
        cassie.execute_sql(&session, "INSERT INTO upsert_tx VALUES (1, 'alpha')", vec![]).unwrap();

        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie.execute_sql(&session, "INSERT INTO upsert_tx VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title", vec![]).unwrap();
        let during = cassie.execute_sql(&session, "SELECT title FROM upsert_tx", vec![]).unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let rolled_back = cassie.execute_sql(&session, "SELECT title FROM upsert_tx", vec![]).unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie.execute_sql(&session, "INSERT INTO upsert_tx VALUES (1, 'gamma') ON CONFLICT (id) DO UPDATE SET title = excluded.title", vec![]).unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let committed = cassie.execute_sql(&session, "SELECT title FROM upsert_tx", vec![]).unwrap();

        // Assert
        assert_eq!(during.rows, vec![vec![Value::String("beta".into())]]);
        assert_eq!(rolled_back.rows, vec![vec![Value::String("alpha".into())]]);
        assert_eq!(committed.rows, vec![vec![Value::String("gamma".into())]]);
        let _ = std::fs::remove_dir_all(path);
    });
}
