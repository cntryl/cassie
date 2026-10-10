use crate::support_relational_qualification::fixture;
use cassie::types::Value;

#[test]
fn should_resolve_update_returning_exists_against_captured_view_and_new_row() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE returning_outer(id BIGINT)",
        "CREATE TABLE returning_inner(id BIGINT)",
        "INSERT INTO returning_outer VALUES(1),(2)",
        "INSERT INTO returning_inner VALUES(1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("update returning setup");
    }

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "UPDATE returning_outer SET id=id WHERE id=2 \
         RETURNING id, EXISTS(SELECT 1 FROM returning_inner) AS any_inner, \
         EXISTS(SELECT 1 FROM returning_inner i WHERE i.id=returning_outer.id) AS correlated",
        vec![],
    );

    // Assert
    let result = result.expect("UPDATE RETURNING resolves both EXISTS forms");
    assert_eq!(
        result.rows,
        vec![vec![Value::Int64(2), Value::Bool(true), Value::Bool(false)]]
    );
    assert_eq!(
        result
            .columns
            .iter()
            .map(|column| (
                column.name.as_str(),
                column.type_oid,
                column.typlen,
                column.atttypmod,
                column.format_code
            ))
            .collect::<Vec<_>>(),
        vec![
            ("id", 20, 8, -1, 0),
            ("any_inner", 16, 1, -1, 0),
            ("correlated", 16, 1, -1, 0)
        ]
    );
}

#[test]
fn should_resolve_insert_select_returning_exists_against_one_captured_view() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE returning_target(id BIGINT)",
        "CREATE TABLE returning_prior(id BIGINT)",
        "BEGIN",
        "INSERT INTO returning_prior VALUES(9),(10)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("insert returning setup");
    }

    // Act
    let inserted = fixture.cassie.execute_sql(
        &fixture.session,
        "INSERT INTO returning_target SELECT id FROM returning_prior WHERE id>=9 ORDER BY id \
         RETURNING id, EXISTS(SELECT 1 FROM returning_prior WHERE id>=9) AS prior_write, \
         EXISTS(SELECT 1 FROM returning_target) AS current_command_write",
        vec![],
    );
    let next_statement = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT EXISTS(SELECT 1 FROM returning_target) AS visible",
        vec![],
    );

    // Assert
    let inserted = inserted.expect("INSERT RETURNING uses the pre-command view");
    assert_eq!(
        inserted.rows,
        vec![
            vec![Value::Int64(9), Value::Bool(true), Value::Bool(false)],
            vec![Value::Int64(10), Value::Bool(true), Value::Bool(false)],
        ]
    );
    assert_eq!(inserted.columns[1].type_oid, 16);
    assert_eq!(inserted.columns[2].type_oid, 16);
    assert_eq!(
        next_statement
            .expect("next statement captures a fresh view")
            .rows,
        vec![vec![Value::Bool(true)]]
    );
    fixture
        .cassie
        .execute_sql(&fixture.session, "ROLLBACK", vec![])
        .expect("rollback staged test transaction");
}

#[test]
fn should_resolve_delete_returning_exists_against_deleted_old_row() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE returning_delete(id BIGINT)",
        "INSERT INTO returning_delete VALUES(1),(2)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("delete returning setup");
    }

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "DELETE FROM returning_delete WHERE id=2 RETURNING id, \
         EXISTS(SELECT 1 FROM returning_delete old WHERE old.id=returning_delete.id) AS was_present",
        vec![],
    );

    // Assert
    let result = result.expect("DELETE RETURNING correlates with the deleted old row");
    assert_eq!(result.rows, vec![vec![Value::Int64(2), Value::Bool(true)]]);
    assert_eq!(result.columns[1].type_oid, 16);
}

#[test]
fn should_use_prior_staged_view_for_update_and_delete_returning_in_transaction() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE returning_tx_update(id BIGINT)",
        "CREATE TABLE returning_tx_delete(id BIGINT)",
        "BEGIN",
        "INSERT INTO returning_tx_update VALUES(1)",
        "INSERT INTO returning_tx_delete VALUES(3)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("transaction snapshot setup");
    }

    // Act
    let updated = fixture.cassie.execute_sql(
        &fixture.session,
        "UPDATE returning_tx_update SET id=2 WHERE id=1 RETURNING id, \
         EXISTS(SELECT 1 FROM returning_tx_update WHERE id=1) AS prior_row, \
         EXISTS(SELECT 1 FROM returning_tx_update WHERE id=2) AS current_write",
        vec![],
    );
    let update_followup = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT EXISTS(SELECT 1 FROM returning_tx_update WHERE id=2)",
        vec![],
    );
    let deleted = fixture.cassie.execute_sql(
        &fixture.session,
        "DELETE FROM returning_tx_delete WHERE id=3 RETURNING id, \
         EXISTS(SELECT 1 FROM returning_tx_delete WHERE id=3) AS prior_row",
        vec![],
    );
    let delete_followup = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT EXISTS(SELECT 1 FROM returning_tx_delete WHERE id=3)",
        vec![],
    );

    // Assert
    assert_eq!(
        updated
            .expect("UPDATE RETURNING reads prior staged view")
            .rows,
        vec![vec![Value::Int64(2), Value::Bool(true), Value::Bool(false)]]
    );
    assert_eq!(
        update_followup
            .expect("next UPDATE command sees fresh view")
            .rows,
        vec![vec![Value::Bool(true)]]
    );
    assert_eq!(
        deleted
            .expect("DELETE RETURNING sees prior staged row")
            .rows,
        vec![vec![Value::Int64(3), Value::Bool(true)]]
    );
    assert_eq!(
        delete_followup
            .expect("next DELETE command sees fresh view")
            .rows,
        vec![vec![Value::Bool(false)]]
    );
    fixture
        .cassie
        .execute_sql(&fixture.session, "ROLLBACK", vec![])
        .expect("rollback staged test transaction");
}

#[test]
fn should_keep_returning_exists_lazy_and_rollback_on_selected_error() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE returning_atomic(id BIGINT)",
        "INSERT INTO returning_atomic VALUES(1),(2)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("atomic returning setup");
    }

    // Act
    let skipped = fixture.cassie.execute_sql(
        &fixture.session,
        "UPDATE returning_atomic SET id=id RETURNING id, \
         COALESCE(true, EXISTS(SELECT 1 FROM returning_atomic WHERE 1/0=0)) AS selected",
        vec![],
    );
    let failed = fixture.cassie.execute_sql(
        &fixture.session,
        "UPDATE returning_atomic SET id=id+10 WHERE id=1 RETURNING id, \
         EXISTS(SELECT 1 FROM returning_atomic i WHERE i.id=1 AND 1/0=0) AS selected",
        vec![],
    );
    let after_error = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id FROM returning_atomic ORDER BY id",
        vec![],
    );

    // Assert
    let mut skipped_rows = skipped.expect("unselected EXISTS branch is lazy").rows;
    skipped_rows.sort_by_key(|row| match row.first() {
        Some(Value::Int64(id)) => *id,
        _ => panic!("expected integer id in UPDATE RETURNING row"),
    });
    assert_eq!(
        skipped_rows,
        vec![
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(true)],
        ]
    );
    assert!(failed
        .expect_err("selected EXISTS error aborts UPDATE RETURNING")
        .to_string()
        .contains("division by zero"));
    assert_eq!(
        after_error.expect("read after failed statement").rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_restore_prior_transaction_writes_after_returning_error_and_savepoint_rollback() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE returning_savepoint(id BIGINT)",
        "BEGIN",
        "INSERT INTO returning_savepoint VALUES(1),(2)",
        "SAVEPOINT before_returning_error",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("savepoint returning setup");
    }

    // Act
    let failed = fixture.cassie.execute_sql(
        &fixture.session,
        "UPDATE returning_savepoint SET id=11 WHERE id=1 RETURNING id, \
         EXISTS(SELECT 1 FROM returning_savepoint WHERE id=1 AND 1/0=0) AS selected",
        vec![],
    );
    let status_after_failure = fixture.session.transaction_status();
    let rejected_read = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id FROM returning_savepoint ORDER BY id",
        vec![],
    );
    let rollback_to = fixture.cassie.execute_sql(
        &fixture.session,
        "ROLLBACK TO SAVEPOINT before_returning_error",
        vec![],
    );
    let status_after_savepoint_rollback = fixture.session.transaction_status();
    let recovered_rows = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id FROM returning_savepoint ORDER BY id",
        vec![],
    );
    let committed = fixture
        .cassie
        .execute_sql(&fixture.session, "COMMIT", vec![]);
    let observer = fixture.cassie.create_session("tester", None);
    let committed_rows = fixture.cassie.execute_sql(
        &observer,
        "SELECT id FROM returning_savepoint ORDER BY id",
        vec![],
    );

    // Assert
    assert!(failed
        .expect_err("selected RETURNING EXISTS error aborts the statement")
        .to_string()
        .contains("division by zero"));
    assert_eq!(status_after_failure, "failed");
    assert!(rejected_read
        .expect_err("failed transaction rejects ordinary reads")
        .to_string()
        .contains("rollback required"));
    rollback_to.expect("rollback to savepoint recovers transaction");
    assert_eq!(status_after_savepoint_rollback, "in_transaction");
    assert_eq!(
        recovered_rows
            .expect("savepoint restores prior staged writes")
            .rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    committed.expect("commit recovered prior writes");
    assert_eq!(
        committed_rows
            .expect("observer sees committed prior writes")
            .rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}
