use cassie::app::Cassie;
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn data_dir(name: &str) -> PathBuf {
    crate::support_temp_dirs::sweep_stale_once();
    std::env::temp_dir().join(format!("cassie-upsert-{name}-{}", Uuid::new_v4()))
}

fn use_local_storage() {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
}

#[test]
fn should_upsert_when_conflict_key_has_a_noncanonical_input_shape() {
    // Arrange
    use_local_storage();
    let path = data_dir("canonical_conflict_keys");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie.execute_sql(&session, "CREATE TABLE canonical_float (k FLOAT UNIQUE, v INT)", vec![]).unwrap();
            cassie.execute_sql(&session, "CREATE TABLE canonical_date (k DATE UNIQUE, v INT)", vec![]).unwrap();
            cassie.execute_sql(&session, "CREATE TABLE canonical_timestamp (k TIMESTAMP UNIQUE, v INT)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO canonical_float VALUES (1.0, 1)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO canonical_date VALUES ('2024-01-01', 1)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO canonical_timestamp VALUES ('2024-01-01T00:00:00Z', 1)", vec![]).unwrap();

            // Act
            let float_update = cassie.execute_sql(&session, "INSERT INTO canonical_float VALUES (1, 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v", vec![]);
            let date_update = cassie.execute_sql(&session, "INSERT INTO canonical_date VALUES ('2024-1-1', 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v", vec![]);
            let timestamp_update = cassie.execute_sql(&session, "INSERT INTO canonical_timestamp VALUES ('2024-01-01 00:00:00', 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v", vec![]);

            // Assert
            assert!(float_update.is_ok(), "FLOAT conflict should update");
            assert!(date_update.is_ok(), "DATE conflict should update");
            assert!(timestamp_update.is_ok(), "TIMESTAMP conflict should update");
            for table in ["canonical_float", "canonical_date", "canonical_timestamp"] {
                let rows = cassie
                    .execute_sql(&session, &format!("SELECT v FROM {table}"), vec![])
                    .unwrap();
                assert_eq!(rows.rows(), vec![vec![Value::Int64(2)]], "table {table}");
            }
            let float_nothing = cassie.execute_sql(&session, "INSERT INTO canonical_float VALUES (1, 3) ON CONFLICT (k) DO NOTHING", vec![]);
            assert!(float_nothing.is_ok(), "FLOAT DO NOTHING should suppress conflict");
            let date_nothing = cassie.execute_sql(&session, "INSERT INTO canonical_date VALUES ('2024-1-1', 3) ON CONFLICT (k) DO NOTHING", vec![]);
            assert!(date_nothing.is_ok(), "DATE DO NOTHING should suppress conflict");
            let timestamp_nothing = cassie.execute_sql(&session, "INSERT INTO canonical_timestamp VALUES ('2024-01-01 00:00:00', 3) ON CONFLICT (k) DO NOTHING", vec![]);
            assert!(timestamp_nothing.is_ok(), "TIMESTAMP DO NOTHING should suppress conflict");
            assert_eq!(float_nothing.unwrap().command(), "INSERT 0 0");
            assert_eq!(date_nothing.unwrap().command(), "INSERT 0 0");
            assert_eq!(timestamp_nothing.unwrap().command(), "INSERT 0 0");
            for table in ["canonical_float", "canonical_date", "canonical_timestamp"] {
                let rows = cassie
                    .execute_sql(&session, &format!("SELECT v FROM {table}"), vec![])
                    .unwrap();
                assert_eq!(rows.rows(), vec![vec![Value::Int64(2)]], "table {table}");
            }

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_update_conflicting_row_given_parameters_excluded_filter_and_returning() {
    // Arrange
    use_local_storage();
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
            assert_eq!(result.command(), "INSERT 0 1");
            assert_eq!(result.rows(), vec![vec![Value::String("one".into()), Value::String("keep".into())]]);
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_reject_invalid_conflict_update_before_mutation() {
    // Arrange
    use_local_storage();
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
            assert_eq!(rows.rows(), vec![vec![Value::String("alpha".into())]]);
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_persist_only_committed_upsert_given_rollback_then_commit() {
    // Arrange
    use_local_storage();
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
            assert_eq!(during.rows(), vec![vec![Value::String("beta".into())]]);
            assert_eq!(rolled_back.rows(), vec![vec![Value::String("alpha".into())]]);
            assert_eq!(committed.rows(), vec![vec![Value::String("gamma".into())]]);
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_reject_upsert_update_of_referenced_key() {
    // Arrange
    use_local_storage();
    let path = data_dir("foreign-key-restrict");
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
                    "CREATE TABLE upsert_restrict_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE upsert_restrict_children (parent_id INT REFERENCES upsert_restrict_parents(id), title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_restrict_parents VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_restrict_children VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie.execute_sql(
                &session,
                "INSERT INTO upsert_restrict_parents VALUES (1, 'alpha') ON CONFLICT (id) DO UPDATE SET id = 2",
                vec![],
            );
            let parents = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM upsert_restrict_parents",
                    vec![],
                )
                .unwrap();

            // Assert
            assert!(result
                .expect_err("referenced key update must be rejected")
                .to_string()
                .contains("still references"));
            assert_eq!(
                parents.rows(),
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_cascade_upsert_update_of_referenced_key() {
    // Arrange
    use_local_storage();
    let path = data_dir("foreign-key-cascade");
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
                    "CREATE TABLE upsert_cascade_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE upsert_cascade_children (parent_id INT, title TEXT, CONSTRAINT upsert_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES upsert_cascade_parents(id) ON UPDATE CASCADE)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_cascade_parents VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_cascade_children VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_cascade_parents VALUES (1, 'alpha') ON CONFLICT (id) DO UPDATE SET id = 2",
                    vec![],
                )
                .unwrap();
            let children = cassie
                .execute_sql(
                    &session,
                    "SELECT parent_id FROM upsert_cascade_children",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(children.rows(), vec![vec![Value::Int64(2)]]);

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_reject_transaction_conflict_resolution_of_referenced_key() {
    // Arrange
    use_local_storage();
    let path = data_dir("foreign-key-transaction-conflict");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let transaction_session = cassie.create_session("transaction", None);
            let concurrent_session = cassie.create_session("concurrent", None);
            cassie
                .execute_sql(
                    &transaction_session,
                    "CREATE TABLE upsert_tx_fk_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &transaction_session,
                    "CREATE TABLE upsert_tx_fk_children (parent_id INT REFERENCES upsert_tx_fk_parents(id), title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&transaction_session, "BEGIN", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &transaction_session,
                    "INSERT INTO upsert_tx_fk_parents VALUES (1, 'staged') ON CONFLICT (id) DO UPDATE SET id = 2",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &concurrent_session,
                    "INSERT INTO upsert_tx_fk_parents VALUES (1, 'committed')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &concurrent_session,
                    "INSERT INTO upsert_tx_fk_children VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            let commit = cassie.execute_sql(&transaction_session, "COMMIT", vec![]);
            let parents = cassie
                .execute_sql(
                    &concurrent_session,
                    "SELECT title FROM upsert_tx_fk_parents",
                    vec![],
                )
                .unwrap();
            let children = cassie
                .execute_sql(
                    &concurrent_session,
                    "SELECT parent_id FROM upsert_tx_fk_children",
                    vec![],
                )
                .unwrap();

            // Assert
            assert!(commit
                .expect_err("commit-time upsert must preserve referential integrity")
                .to_string()
                .contains("still references"));
            assert_eq!(
                parents.rows(),
                vec![vec![Value::String("committed".to_string())]]
            );
            assert_eq!(children.rows(), vec![vec![Value::Int64(1)]]);

            let _ = std::fs::remove_dir_all(path);
        });
}
