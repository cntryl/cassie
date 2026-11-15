//! SQL surface and policy selection of `cassie.projection_write_mode` (#1279).

use crate::app::{Cassie, ProjectionCommitPolicy};
use crate::config::CassieRuntimeConfig;

fn sql_fixture() -> (Cassie, crate::app::CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("projector", None);
    for sql in [
        "CREATE TABLE replay_sql_data (value INT)",
        "CREATE TABLE replay_sql_checkpoint (position INT)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    (cassie, session)
}

fn execute(cassie: &Cassie, session: &crate::app::CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn declare_replayable(cassie: &Cassie, session: &crate::app::CassieSession) {
    execute(cassie, session, "BEGIN");
    execute(
        cassie,
        session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
}

#[test]
fn should_reject_projection_mode_outside_explicit_transaction() {
    // Arrange
    let (cassie, session) = sql_fixture();

    // Act
    let local = cassie.execute_sql(
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
        vec![],
    );
    let session_setting = cassie.execute_sql(
        &session,
        "SET cassie.projection_write_mode = 'replayable'",
        vec![],
    );

    // Assert
    assert_eq!(local.unwrap_err().sql_state(), "25P01");
    assert!(session_setting.is_err());
}

#[test]
fn should_reject_projection_mode_declared_after_write() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "INSERT INTO replay_sql_data (value) VALUES (7)",
    );

    // Act
    let result = cassie.execute_sql(
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
        vec![],
    );

    // Assert
    assert_eq!(result.unwrap_err().sql_state(), "25000");
}

#[test]
fn should_prevent_set_config_from_creating_projector_session_mode() {
    // Arrange
    let (cassie, session) = sql_fixture();

    // Act
    let result = cassie.execute_sql(
        &session,
        "SELECT set_config('cassie.projection_write_mode', 'replayable', false)",
        vec![],
    );
    let displayed = cassie
        .execute_sql(&session, "SHOW cassie.projection_write_mode", vec![])
        .unwrap();

    // Assert
    assert!(result.is_err());
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("default".to_owned())]]
    );
}

#[test]
fn should_reset_projector_mode_after_transaction_completion() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
    execute(&cassie, &session, "COMMIT");

    // Act
    let displayed = cassie
        .execute_sql(&session, "SHOW cassie.projection_write_mode", vec![])
        .unwrap();

    // Assert
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("default".to_owned())]]
    );
}

#[test]
fn should_restore_default_projector_mode_via_local_default() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );

    // Act
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = DEFAULT",
    );
    let displayed = cassie
        .execute_sql(&session, "SHOW cassie.projection_write_mode", vec![])
        .unwrap();

    // Assert
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("default".to_owned())]]
    );
}

#[test]
fn should_reject_projector_mode_in_startup_settings() {
    // Arrange
    let (_, session) = sql_fixture();

    // Act
    let result = session.set_startup_setting("cassie.projection_write_mode", "replayable");

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_invalid_projector_mode_value() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");

    // Act
    let result = cassie.execute_sql(
        &session,
        "SET LOCAL cassie.projection_write_mode = 'fast'",
        vec![],
    );

    // Assert
    assert_eq!(result.unwrap_err().sql_state(), "22023");
}

#[test]
fn should_preserve_projector_mode_after_reset_all() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );

    // Act
    execute(&cassie, &session, "RESET ALL");
    let displayed = cassie
        .execute_sql(&session, "SHOW cassie.projection_write_mode", vec![])
        .unwrap();

    // Assert
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("replayable".to_owned())]]
    );
}

#[test]
fn should_select_replayable_policy_for_declared_row_only_transaction() {
    // Arrange
    let (cassie, session) = sql_fixture();
    declare_replayable(&cassie, &session);
    execute(&cassie, &session, "INSERT INTO replay_sql_data (value) VALUES (7)");
    execute(
        &cassie,
        &session,
        "UPDATE replay_sql_checkpoint SET position = 1",
    );

    // Act
    let policy = session.projection_commit_policy(false);
    execute(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(policy, ProjectionCommitPolicy::Replayable);
}

#[test]
fn should_keep_existing_policy_for_undeclared_transaction() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "INSERT INTO replay_sql_data (value) VALUES (7)");

    // Act
    let policy = session.projection_commit_policy(false);
    execute(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(policy, ProjectionCommitPolicy::Existing);
}

#[test]
fn should_select_strict_policy_for_declared_strict_transaction() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'strict'",
    );
    execute(&cassie, &session, "INSERT INTO replay_sql_data (value) VALUES (7)");

    // Act
    let policy = session.projection_commit_policy(false);
    execute(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(policy, ProjectionCommitPolicy::Strict);
}

#[test]
fn should_promote_declared_transaction_with_ddl_to_existing_policy() {
    // Arrange
    let (cassie, session) = sql_fixture();
    declare_replayable(&cassie, &session);
    execute(&cassie, &session, "CREATE TABLE replay_sql_mixed (value INT)");
    execute(&cassie, &session, "INSERT INTO replay_sql_data (value) VALUES (7)");

    // Act
    let policy = session.projection_commit_policy(false);
    execute(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(policy, ProjectionCommitPolicy::Existing);
}

#[test]
fn should_keep_sequence_promotion_sticky_after_savepoint_rollback() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "CREATE SEQUENCE replay_sql_mode_sequence");
    declare_replayable(&cassie, &session);
    execute(&cassie, &session, "SAVEPOINT before_sequence");
    execute(
        &cassie,
        &session,
        "SELECT nextval('replay_sql_mode_sequence')",
    );
    execute(&cassie, &session, "ROLLBACK TO SAVEPOINT before_sequence");
    execute(&cassie, &session, "INSERT INTO replay_sql_data (value) VALUES (7)");

    // Act
    let policy = session.projection_commit_policy(false);
    execute(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(policy, ProjectionCommitPolicy::Existing);
}

#[test]
fn should_keep_declaration_window_closed_after_savepoint_rollback_of_first_write() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "SAVEPOINT before_write");
    execute(&cassie, &session, "INSERT INTO replay_sql_data (value) VALUES (7)");
    execute(&cassie, &session, "ROLLBACK TO SAVEPOINT before_write");

    // Act
    let result = cassie.execute_sql(
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
        vec![],
    );

    // Assert
    assert_eq!(result.unwrap_err().sql_state(), "25000");
}

#[test]
fn should_start_chained_transaction_with_default_mode() {
    // Arrange
    let (cassie, session) = sql_fixture();
    declare_replayable(&cassie, &session);

    // Act
    execute(&cassie, &session, "COMMIT AND CHAIN");
    let displayed = cassie
        .execute_sql(&session, "SHOW cassie.projection_write_mode", vec![])
        .unwrap();
    execute(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("default".to_owned())]]
    );
}

#[test]
fn should_reset_mode_after_rollback() {
    // Arrange
    let (cassie, session) = sql_fixture();
    declare_replayable(&cassie, &session);

    // Act
    execute(&cassie, &session, "ROLLBACK");
    let policy = session.projection_commit_policy(false);
    let displayed = cassie
        .execute_sql(&session, "SHOW cassie.projection_write_mode", vec![])
        .unwrap();

    // Assert
    assert_eq!(policy, ProjectionCommitPolicy::Existing);
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("default".to_owned())]]
    );
}

#[test]
fn should_expose_declared_mode_through_current_setting() {
    // Arrange
    let (cassie, session) = sql_fixture();
    declare_replayable(&cassie, &session);

    // Act
    let displayed = cassie
        .execute_sql(
            &session,
            "SELECT current_setting('cassie.projection_write_mode')",
            vec![],
        )
        .unwrap();
    execute(&cassie, &session, "ROLLBACK");

    // Assert
    assert_eq!(
        displayed.rows(),
        &[vec![crate::types::Value::String("replayable".to_owned())]]
    );
}

#[test]
fn should_reject_targeted_reset_of_projector_mode() {
    // Arrange
    let (cassie, session) = sql_fixture();
    declare_replayable(&cassie, &session);

    // Act
    let result = cassie.execute_sql(&session, "RESET cassie.projection_write_mode", vec![]);

    // Assert
    assert!(result.is_err());
}
