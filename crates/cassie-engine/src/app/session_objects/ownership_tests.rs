//! Shared object leaves reject stale roots without relying on statement entry.

use super::*;
use crate::app::FailureAuthority;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::sql::ast::QueryStatement;
use crate::types::Value;
use std::time::Instant;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("objects_owner", None);
    for sql in [
        "CREATE TABLE objects_rows (id INT)",
        "INSERT INTO objects_rows VALUES (1), (2)",
        "CREATE SEQUENCE objects_seq",
    ] {
        cassie.execute_sql(&session, sql, Vec::new()).expect(sql);
    }
    (cassie, session)
}

fn controls(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, Vec::new()).expect(sql);
}

fn leaf(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, CassieError> {
    let parsed = crate::sql::parse_statement(sql).expect("session statement");
    let QueryStatement::SessionObject(statement) = parsed.statement else {
        panic!("session object statement")
    };
    cassie.execute_session_object(
        session,
        &statement,
        sql,
        SessionObjectCall {
            controls,
            authority: session.operation_failure_authority(),
            mode: ExecutionMode::SimpleQuery,
            wire_output: false,
        },
    )
}

#[test]
fn should_preserve_replacement_object_state_when_an_old_leaf_resumes() {
    for detached in [false, true] {
        for sql in [
            "PREPARE late AS SELECT 9",
            "DEALLOCATE kept",
            "DEALLOCATE ALL",
            "DECLARE late_cursor CURSOR FOR SELECT id FROM objects_rows",
            "FETCH NEXT FROM kept_cursor",
            "MOVE NEXT FROM kept_cursor",
            "CLOSE kept_cursor",
            "CLOSE ALL",
        ] {
            // Arrange
            let (cassie, session) = fixture();
            execute(&cassie, &session, "BEGIN");
            let authority = FailureAuthority::capture(&session);
            let operation = session.with_operation_failure_authority(Some(&authority));
            let batch = detached.then(|| {
                cassie
                    .new_statement_batch(Some(&operation))
                    .expect("fork A")
            });
            let old = batch.as_ref().map_or(&operation, |batch| batch.session());
            execute(&cassie, &session, "ROLLBACK");
            execute(&cassie, &session, "BEGIN");
            execute(&cassie, &session, "PREPARE kept AS SELECT 7");
            execute(
                &cassie,
                &session,
                "DECLARE kept_cursor SCROLL CURSOR FOR SELECT id FROM objects_rows ORDER BY id",
            );
            let owner = session.transaction_unique_owner_id();
            // Act
            let result = leaf(&cassie, old, sql, &controls(1 << 20));
            // Assert
            assert_eq!(
                result.expect_err(sql).sql_state(),
                "25000",
                "{sql}; detached={detached}"
            );
            assert_eq!(session.transaction_unique_owner_id(), owner);
            assert_eq!(session.transaction_status(), "in_transaction");
            session.with_session_objects(|objects| {
                assert!(objects.prepared("kept").is_some());
                assert!(objects.prepared("late").is_none());
                assert_eq!(objects.cursor("kept_cursor").expect("B cursor").position, 0);
                assert!(objects.cursor("late_cursor").is_none());
            });
        }
    }
}

#[test]
fn should_preserve_replacement_session_state_from_old_discard() {
    // Arrange
    let (cassie, session) = fixture();
    let authority = FailureAuthority::capture(&session);
    let old = session.with_operation_failure_authority(Some(&authority));
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "PREPARE kept AS SELECT 7");
    execute(
        &cassie,
        &session,
        "DECLARE kept_cursor CURSOR WITH HOLD FOR SELECT id FROM objects_rows",
    );
    execute(&cassie, &session, "SET application_name = 'newer'");
    execute(&cassie, &session, "SELECT nextval('objects_seq')");
    execute(&cassie, &session, "COMMIT");
    // Act
    let result = leaf(&cassie, &old, "DISCARD ALL", &controls(1 << 20));
    // Assert
    assert_eq!(result.expect_err("stale DISCARD").sql_state(), "25000");
    session.with_session_objects(|objects| {
        assert!(objects.prepared("kept").is_some());
        assert!(objects.cursor("kept_cursor").is_some());
    });
    assert_eq!(
        session.setting("application_name").expect("setting"),
        "newer"
    );
    let value = cassie
        .execute_sql(&session, "SELECT currval('objects_seq')", Vec::new())
        .expect("B history");
    assert_eq!(value.rows()[0][0], Value::Int64(1));
}

#[test]
fn should_admit_discard_output_before_destroying_same_owner_state() {
    // Arrange
    let (cassie, session) = fixture();
    execute(&cassie, &session, "PREPARE kept AS SELECT 7");
    execute(&cassie, &session, "SELECT nextval('objects_seq')");
    session
        .set_setting("application_name", "kept")
        .expect("setting");
    let authority = FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    // Act
    let denied = leaf(&cassie, &operation, "DISCARD ALL", &controls(0));
    let retained = session.with_session_objects(|objects| objects.prepared("kept").is_some());
    let current = session
        .setting("application_name")
        .expect("retained setting");
    leaf(&cassie, &operation, "DISCARD ALL", &controls(1 << 20)).expect("same owner discard");
    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    assert!(retained);
    assert_eq!(current, "kept");
    session.with_session_objects(|objects| assert_eq!(objects.prepared_count(), 0));
    assert_eq!(
        session.setting("application_name").expect("reset setting"),
        ""
    );
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT currval('objects_seq')", Vec::new())
            .expect_err("history reset")
            .sql_state(),
        "55000"
    );
}

#[test]
fn should_fence_protocol_namespace_writes_by_owner() {
    // Arrange
    let (cassie, session) = fixture();
    execute(&cassie, &session, "BEGIN");
    let authority = FailureAuthority::capture(&session);
    let old = session.with_operation_failure_authority(Some(&authority));
    execute(&cassie, &session, "ROLLBACK");
    execute(&cassie, &session, "BEGIN");
    session
        .register_wire_prepared("kept", 2, "SELECT 2")
        .expect("current connection");
    // Act
    let insert = old.register_wire_prepared("late", 3, "SELECT 3");
    let remove = old.forget_wire_prepared("kept", 2);
    session
        .forget_wire_prepared("kept", 1)
        .expect("wrong-id cleanup");
    // Assert
    assert_eq!(insert.expect_err("old registration").sql_state(), "25000");
    assert_eq!(remove.expect_err("old close").sql_state(), "25000");
    assert!(session.has_wire_prepared("kept", 2));
    assert!(!session.has_wire_prepared("late", 3));
    session
        .forget_wire_prepared("kept", 2)
        .expect("matching cleanup");
    assert!(!session.has_wire_prepared("kept", 2));
}

#[test]
fn should_reject_shared_object_mutation_from_a_fork_without_lineage() {
    // Arrange
    let (cassie, session) = fixture();
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "PREPARE kept AS SELECT 7");
    let batch = cassie
        .new_statement_batch(Some(&session))
        .expect("fork automatically captures its root lineage");
    let missing = batch.session().without_operation_failure_authority();
    // Act
    let result = missing.with_owned_session_objects(|objects, _, _| {
        objects.clear_prepared();
        Ok(())
    });
    // Assert
    assert_eq!(
        result.expect_err("missing fork lineage").sql_state(),
        "25000"
    );
    assert!(session.has_sql_prepared("kept"));
}

#[test]
fn should_access_shared_objects_through_a_current_detached_owner() {
    // Arrange
    let (cassie, session) = fixture();
    execute(&cassie, &session, "BEGIN");
    let authority = FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    let batch = cassie
        .new_statement_batch(Some(&operation))
        .expect("current fork");
    let query = controls(1 << 20);
    // Act
    leaf(
        &cassie,
        batch.session(),
        "PREPARE fork_prepared AS SELECT 7",
        &query,
    )
    .expect("owned PREPARE");
    leaf(
        &cassie,
        batch.session(),
        "DECLARE fork_cursor CURSOR FOR SELECT id FROM objects_rows ORDER BY id",
        &query,
    )
    .expect("owned DECLARE");
    let first = leaf(
        &cassie,
        batch.session(),
        "FETCH NEXT FROM fork_cursor",
        &query,
    )
    .expect("owned FETCH");
    // Assert
    assert_eq!(first.rows(), &[vec![Value::Int64(1)]]);
    session.with_session_objects(|objects| {
        assert!(objects.prepared("fork_prepared").is_some());
        let cursor = objects.cursor("fork_cursor").expect("root-owned cursor");
        assert_eq!(cursor.position, 1);
        assert!(cursor.in_transaction);
    });
    execute(&cassie, &session, "COMMIT");
    assert!(session.has_sql_prepared("fork_prepared"));
    session.with_session_objects(|objects| assert!(objects.cursor("fork_cursor").is_none()));
}

#[test]
fn should_refuse_a_sealed_fork_while_preserving_the_committing_root() {
    // Arrange
    let (cassie, session) = fixture();
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "PREPARE kept AS SELECT 7");
    let authority = FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    let batch = cassie
        .new_statement_batch(Some(&operation))
        .expect("fork before seal");
    let seal = session.seal_transaction_for_commit();
    // Act
    let fork = batch.session().with_owned_session_objects(|objects, _, _| {
        objects.clear_prepared();
        Ok(())
    });
    let kept = session.has_sql_prepared("kept");
    let root = operation.with_owned_session_objects(|objects, _, _| {
        objects.clear_prepared();
        Ok(())
    });
    drop(seal);
    // Assert
    assert_eq!(
        fork.expect_err("fork cannot enter sealed root").sql_state(),
        "25000"
    );
    assert!(kept);
    root.expect("committing root retains its owned permission");
    assert!(!session.has_sql_prepared("kept"));
}

#[test]
fn should_publish_a_held_cursor_after_its_own_modifying_cte_commits() {
    // Arrange
    let (cassie, session) = fixture();
    // Act
    execute(&cassie, &session,
        "DECLARE held CURSOR WITH HOLD FOR WITH changed AS (DELETE FROM objects_rows RETURNING id) SELECT id FROM changed ORDER BY id");
    let fetched = cassie
        .execute_sql(&session, "FETCH ALL FROM held", Vec::new())
        .expect("retained held result");
    let rows = cassie
        .execute_sql(&session, "SELECT count(*) FROM objects_rows", Vec::new())
        .expect("committed deletion");
    // Assert
    assert_eq!(
        fetched.rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert_eq!(rows.rows(), &[vec![Value::Int64(0)]]);
    assert_eq!(session.transaction_status(), "idle");
}

#[test]
fn should_revoke_an_owned_idle_receipt_after_an_independent_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    execute(&cassie, &operation, "BEGIN");
    execute(&cassie, &operation, "COMMIT");
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "COMMIT");
    // Act
    let result = leaf(
        &cassie,
        &operation,
        "PREPARE late AS SELECT 9",
        &controls(1 << 20),
    );
    // Assert
    assert_eq!(result.expect_err("B revoked receipt").sql_state(), "25000");
    assert!(!session.has_sql_prepared("late"));
}

#[test]
fn should_not_treat_another_operations_commit_as_its_own_retirement() {
    // Arrange
    let (cassie, session) = fixture();
    execute(&cassie, &session, "BEGIN");
    let authority = FailureAuthority::capture(&session);
    let old = session.with_operation_failure_authority(Some(&authority));
    execute(&cassie, &session, "COMMIT");
    // Act
    let result = leaf(
        &cassie,
        &old,
        "PREPARE late AS SELECT 9",
        &controls(1 << 20),
    );
    // Assert
    assert_eq!(result.expect_err("unowned retirement").sql_state(), "25000");
    assert!(!session.has_sql_prepared("late"));
}

#[test]
fn should_not_transfer_the_roots_idle_retirement_receipt_to_a_detached_fork() {
    // Arrange
    let (cassie, session) = fixture();
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    execute(&cassie, &operation, "BEGIN");
    let batch = cassie
        .new_statement_batch(Some(&operation))
        .expect("fork before retirement");
    execute(&cassie, &operation, "COMMIT");
    // Act
    let result = batch.session().with_owned_session_objects(|objects, _, _| {
        objects.clear_prepared();
        Ok(())
    });
    // Assert
    assert_eq!(
        result
            .expect_err("fork has no root retirement permission")
            .sql_state(),
        "25000"
    );
}
