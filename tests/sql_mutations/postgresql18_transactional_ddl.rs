//! PostgreSQL 18.6 transactional catalog publication tracked by #833.

use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_restore_transactional_catalog_at_savepoint_rollback() {
    // Arrange
    let fixture = sql_fixture("issue-833-ddl-savepoint", &[]);
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("CREATE TABLE tx_ddl_savepoint_kept (id INT PRIMARY KEY)")
        .expect("create table before savepoint");
    fixture
        .execute("SAVEPOINT before_second_ddl")
        .expect("create savepoint");
    fixture
        .execute("CREATE TABLE tx_ddl_savepoint_rolled_back (id INT PRIMARY KEY)")
        .expect("create table after savepoint");

    // Act
    fixture
        .execute("ROLLBACK TO SAVEPOINT before_second_ddl")
        .expect("roll back catalog changes after savepoint");
    fixture
        .execute("COMMIT")
        .expect("commit recovered transaction");
    let visible_tables = fixture.rows(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' \
           AND table_name LIKE 'tx_ddl_savepoint_%' \
         ORDER BY table_name",
    );

    // Assert
    assert_eq!(
        visible_tables,
        vec![vec![Value::String("tx_ddl_savepoint_kept".to_owned())]]
    );
}

#[test]
fn should_restore_dropped_table_after_transaction_rollback() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-drop-rollback",
        &[
            "CREATE TABLE tx_ddl_drop_restore (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO tx_ddl_drop_restore VALUES (1, 'retained')",
        ],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("DROP TABLE tx_ddl_drop_restore")
        .expect("drop table transactionally");

    // Act
    fixture.execute("ROLLBACK").expect("roll back table drop");
    let rows = fixture.rows("SELECT id, label FROM tx_ddl_drop_restore ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::String("retained".to_owned())]]
    );
}
