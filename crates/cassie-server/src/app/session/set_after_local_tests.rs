//! A plain `SET` or `RESET` in a transaction outlives `SET LOCAL` of the same setting.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("setting_owner", None);
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for statement in statements {
        cassie
            .execute_sql(session, statement, Vec::new())
            .unwrap_or_else(|error| panic!("{statement} failed: {error}"));
    }
}

fn application_name(session: &CassieSession) -> String {
    session.setting("application_name").expect("setting")
}

#[test]
fn should_keep_plain_set_after_set_local_at_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET LOCAL application_name = 'a'",
            "SET application_name = 'b'",
            "COMMIT",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "b");
}

#[test]
fn should_keep_plain_set_before_set_local_at_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET application_name = 'b'",
            "SET LOCAL application_name = 'c'",
            "COMMIT",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "b");
}

#[test]
fn should_restore_default_when_reset_follows_set_local_at_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET LOCAL application_name = 'a'",
            "RESET application_name",
            "COMMIT",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "");
}

#[test]
fn should_keep_reset_all_before_set_local_at_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &["RESET ALL", "SET LOCAL application_name = 'a'", "COMMIT"],
    );
    // Assert
    assert_eq!(application_name(&session), "");
}

#[test]
fn should_show_plain_set_after_set_local_inside_the_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, &["BEGIN"]);
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET LOCAL application_name = 'a'",
            "SET application_name = 'b'",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "b");
}

#[test]
fn should_show_set_local_after_plain_set_inside_the_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, &["BEGIN"]);
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET application_name = 'b'",
            "SET LOCAL application_name = 'c'",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "c");
}

#[test]
fn should_restore_pre_transaction_value_on_rollback_after_mixed_sets() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET LOCAL application_name = 'a'",
            "SET application_name = 'b'",
            "ROLLBACK",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "pre");
}

#[test]
fn should_keep_plain_set_after_rollback_to_savepoint_at_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET application_name = 'x'",
            "SAVEPOINT s",
            "SET LOCAL application_name = 'y'",
            "ROLLBACK TO s",
            "SET application_name = 'z'",
            "COMMIT",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "z");
}

#[test]
fn should_discard_plain_set_rolled_back_to_savepoint_at_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        &["SET application_name = 'pre'", "BEGIN"],
    );
    // Act
    run(
        &cassie,
        &session,
        &[
            "SET LOCAL application_name = 'l'",
            "SAVEPOINT s",
            "SET application_name = 'p'",
            "ROLLBACK TO s",
            "COMMIT",
        ],
    );
    // Assert
    assert_eq!(application_name(&session), "pre");
}
