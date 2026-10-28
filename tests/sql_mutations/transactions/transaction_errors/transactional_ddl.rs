#[test]
fn should_publish_transactional_table_creation_only_after_commit() {
    // Arrange
    let fixture = fixture("tx-ddl-visibility");
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin transaction");

    // Act
    fixture
        .execute("CREATE TABLE tx_ddl_pending (id INT PRIMARY KEY, label TEXT)")
        .expect("create table transactionally");
    fixture
        .execute("INSERT INTO tx_ddl_pending (id, label) VALUES (1, 'private')")
        .expect("write through uncommitted table");
    let private = fixture
        .execute("SELECT id, label FROM tx_ddl_pending")
        .expect("read own transactional DDL");
    let invisible =
        fixture
            .cassie
            .execute_sql(&observer, "SELECT id, label FROM tx_ddl_pending", vec![]);
    fixture
        .execute("COMMIT")
        .expect("publish transactional DDL");
    let published = fixture
        .cassie
        .execute_sql(&observer, "SELECT id, label FROM tx_ddl_pending", vec![])
        .expect("read the committed table from another session");

    // Assert
    assert_eq!(
        private.rows(),
        vec![vec![Value::Int64(1), Value::String("private".into())]]
    );
    assert!(invisible.is_err(), "uncommitted table must not be visible");
    assert_eq!(
        published.rows(),
        vec![vec![Value::Int64(1), Value::String("private".into())]]
    );
}

#[test]
fn should_restore_transactional_table_alteration_after_rollback() {
    // Arrange
    let fixture = fixture("tx-ddl-rollback");
    fixture.execute("BEGIN").expect("begin transaction");

    // Act
    fixture
        .execute("ALTER TABLE tx_error_items ADD COLUMN transient TEXT")
        .expect("alter table transactionally");
    fixture.execute("ROLLBACK").expect("rollback DDL");
    let absent = fixture.execute("SELECT transient FROM tx_error_items");

    // Assert
    assert!(
        absent.is_err(),
        "rolled-back column must not remain in schema"
    );
    assert_eq!(fixture.session.transaction_status(), "idle");
}
