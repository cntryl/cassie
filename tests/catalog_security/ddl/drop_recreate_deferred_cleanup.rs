//! Deferred DROP cleanup must only remove the retired relation, never a table
//! later created under the same name or index metadata that followed a rename.
use crate::support_sql_fixture::sql_fixture;
use cassie::app::Cassie;
use cassie::types::Value;

#[test]
fn should_keep_recreated_table_rows_after_deferred_drop_cleanup_runs() {
    // Arrange
    let fixture = sql_fixture(
        "recreate-deferred-drop-rows",
        &[
            "CREATE TABLE t (a INT, b TEXT)",
            "INSERT INTO t VALUES (1, 'old')",
        ],
    );
    let reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("DROP TABLE t").unwrap();
    fixture.execute("CREATE TABLE t (c INT, d INT)").unwrap();
    fixture.execute("INSERT INTO t VALUES (5, 6)").unwrap();

    // Act
    drop(reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();
    let rows = fixture.rows("SELECT c, d FROM t");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(5), Value::Int64(6)]]);
}

#[test]
fn should_use_new_schema_for_table_recreated_during_deferred_drop() {
    // Arrange
    let fixture = sql_fixture(
        "recreate-deferred-drop-schema",
        &[
            "CREATE TABLE t (a INT, b TEXT)",
            "INSERT INTO t VALUES (1, 'old')",
        ],
    );
    let reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("DROP TABLE t").unwrap();
    fixture.execute("CREATE TABLE t (c INT, d INT)").unwrap();
    drop(reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();

    // Act
    let count = fixture.rows("SELECT count(*) FROM t");
    let old_column = fixture.execute("SELECT a FROM t");

    // Assert
    assert_eq!(count, vec![vec![Value::Int64(0)]]);
    assert!(old_column.is_err(), "old column must not survive");
}

#[test]
fn should_keep_recreated_table_after_restart_with_deferred_drop() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("recreate-deferred-restart");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE t (a INT, b TEXT)",
        "INSERT INTO t VALUES (1, 'old')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    let reader = cassie.begin_schema_epoch_guard_for_diagnostics();
    for sql in [
        "DROP TABLE t",
        "CREATE TABLE t (c INT, d INT)",
        "INSERT INTO t VALUES (5, 6)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    drop(reader);
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("startup");
    let session = restarted.create_session("tester", None);
    let rows = restarted
        .execute_sql(&session, "SELECT c, d FROM t", vec![])
        .unwrap();

    // Assert
    assert_eq!(rows.rows(), vec![vec![Value::Int64(5), Value::Int64(6)]]);
    let _ = std::fs::remove_dir_all(path);
}
