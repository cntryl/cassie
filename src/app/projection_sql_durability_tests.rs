use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::observe_native_commits;

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

#[test]
fn should_use_buffered_wal_for_declared_sql_projection_transaction() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
    execute(
        &cassie,
        &session,
        "INSERT INTO replay_sql_data (value) VALUES (7)",
    );
    execute(
        &cassie,
        &session,
        "INSERT INTO replay_sql_checkpoint (position) VALUES (1)",
    );

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.execute_sql(&session, "COMMIT", vec![]));

    // Assert
    result.unwrap();
    assert_eq!(writes.first_policy, Some("local_buffered"));
    assert!(writes.local_buffered_attempts > 0);
    assert_eq!(writes.best_effort_attempts, 0);
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT value FROM replay_sql_data", vec![])
            .unwrap()
            .rows()
            .len(),
        1
    );
    assert_eq!(
        cassie
            .execute_sql(
                &session,
                "SELECT position FROM replay_sql_checkpoint",
                vec![]
            )
            .unwrap()
            .rows()
            .len(),
        1
    );
}

#[test]
fn should_use_strict_policy_for_mixed_projection_transaction() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
    execute(
        &cassie,
        &session,
        "CREATE TABLE replay_sql_mixed (value INT)",
    );
    execute(
        &cassie,
        &session,
        "INSERT INTO replay_sql_data (value) VALUES (7)",
    );

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.execute_sql(&session, "COMMIT", vec![]));

    // Assert
    result.unwrap();
    assert_eq!(writes.first_policy, Some("local_sync"));
    assert!(writes.local_sync_attempts > 0);
    assert_eq!(writes.local_buffered_attempts, 0);
    assert_eq!(writes.best_effort_attempts, 0);
}

#[test]
fn should_restore_default_policy_after_savepoint_rollback() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "SAVEPOINT before_mode");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
    execute(&cassie, &session, "ROLLBACK TO SAVEPOINT before_mode");
    execute(
        &cassie,
        &session,
        "INSERT INTO replay_sql_data (value) VALUES (7)",
    );

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.execute_sql(&session, "COMMIT", vec![]));

    // Assert
    result.unwrap();
    assert!(writes.local_sync_attempts > 0);
    assert_eq!(writes.local_buffered_attempts, 0);
}

#[test]
fn should_retain_sequence_effect_classification_after_savepoint_rollback() {
    // Arrange
    let (cassie, session) = sql_fixture();
    execute(&cassie, &session, "CREATE SEQUENCE replay_sql_sequence");
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
    execute(&cassie, &session, "SAVEPOINT before_sequence");
    execute(&cassie, &session, "SELECT nextval('replay_sql_sequence')");
    execute(&cassie, &session, "ROLLBACK TO SAVEPOINT before_sequence");
    execute(
        &cassie,
        &session,
        "INSERT INTO replay_sql_data (value) VALUES (7)",
    );

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.execute_sql(&session, "COMMIT", vec![]));

    // Assert
    result.unwrap();
    assert_eq!(writes.first_policy, Some("local_sync"));
    assert_eq!(writes.local_buffered_attempts, 0);
    assert_eq!(writes.best_effort_attempts, 0);
}

