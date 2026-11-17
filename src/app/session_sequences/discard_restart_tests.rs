//! DISCARD forgets session sequence history without losing rollback work.

use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::SequenceCursor;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn should_preserve_sequence_history_when_an_old_operation_cannot_reset_the_replacement() {
    // Arrange
    let session = CassieSession::new("old_sequence_reset".into(), None);
    session.begin_transaction(None).unwrap();
    let operation = session.with_new_wire_operation();
    let _authority = crate::app::FailureAuthority::capture_for_operation(&operation);
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
    session.record_sequence_value("replacement", 7, 42, true);

    // Act
    let reset = operation.discard_sequence_values_for_operation();

    // Assert
    assert_eq!(reset.unwrap_err().sql_state(), "25000");
    let values = session.sequence_values.lock();
    assert_eq!(values.by_sequence.get("id:7"), Some(&42));
    assert_eq!(values.last, Some(("replacement".into(), 7)));
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_clear_owned_sequence_history_only_after_the_owned_reset_leaf_succeeds() {
    // Arrange
    let session = CassieSession::new("owned_sequence_reset".into(), None);
    let operation = session.with_new_wire_operation();
    let _authority = crate::app::FailureAuthority::capture_for_operation(&operation);
    session.record_sequence_value("owned", 7, 42, true);

    // Act
    let denied: Result<(), CassieError> = operation.with_owned_sequence_reset(|_, _| {
        Err(CassieError::InvalidParameterValue(
            "reset leaf refused".into(),
        ))
    });
    let retained = session
        .sequence_values
        .lock()
        .by_sequence
        .get("id:7")
        .copied();
    let accepted = operation.discard_sequence_values_for_operation();

    // Assert
    assert!(denied.is_err());
    assert_eq!(retained, Some(42));
    accepted.unwrap();
    let values = session.sequence_values.lock();
    assert!(values.by_sequence.is_empty());
    assert!(values.last.is_none());
}

fn fixture() -> (Cassie, CassieSession, String, SequenceCursor) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE discard_restart_probe (id SERIAL PRIMARY KEY, payload TEXT)",
            vec![],
        )
        .expect("committed identity table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO discard_restart_probe (payload) VALUES ('retained')",
            vec![],
        )
        .expect("committed row");
    let sequence = cassie
        .catalog
        .list_sequences()
        .into_iter()
        .find(|sequence| {
            sequence.owned_by.as_ref().is_some_and(|owner| {
                crate::catalog::name_matches(&owner.table, "discard_restart_probe")
            })
        })
        .expect("owned sequence");
    assert!(!cassie
        .midge
        .is_private_sequence(&sequence)
        .expect("native sequence route"));
    cassie
        .execute_sql(
            &session,
            &format!("SELECT setval('{}', 39, true)", sequence.name),
            vec![],
        )
        .expect("seed cursor");
    let advanced = cassie
        .execute_sql(
            &session,
            &format!("SELECT nextval('{}')", sequence.name),
            vec![],
        )
        .expect("seed session history");
    assert_eq!(advanced.rows(), &[vec![Value::Int64(40)]]);
    let prior = cursor(&cassie, &sequence.name);
    (cassie, session, sequence.name, prior)
}

fn cursor(cassie: &Cassie, name: &str) -> SequenceCursor {
    cassie
        .midge
        .get_sequence(name)
        .expect("read native sequence")
        .expect("sequence exists")
        .cursor()
}

fn assert_history_missing(cassie: &Cassie, session: &CassieSession, name: &str) {
    for sql in [
        format!("SELECT currval('{name}')"),
        "SELECT lastval()".to_owned(),
    ] {
        let result = cassie.execute_sql(session, &sql, vec![]);
        let error = result.expect_err("discarded history is unavailable");
        assert_eq!(error.descriptor().sql_state, "55000", "{sql}: {error}");
    }
}

fn assert_history_is_40(cassie: &Cassie, session: &CassieSession, name: &str) {
    for sql in [
        format!("SELECT currval('{name}')"),
        "SELECT lastval()".to_owned(),
    ] {
        let result = cassie
            .execute_sql(session, &sql, vec![])
            .expect("sequence history");
        assert_eq!(result.rows(), &[vec![Value::Int64(40)]]);
    }
}

#[test]
fn should_preserve_restart_when_discarding_history_in_failed_transaction() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE discard_restart_probe RESTART IDENTITY",
            vec![],
        )
        .expect("restart");
    // The restart is staged with the transaction, not committed natively.
    assert_eq!(cursor(&cassie, &name), prior);
    session.mark_transaction_failed();

    // Act
    session.discard_sequence_values();
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");

    // Assert
    assert_eq!(cursor(&cassie, &name), prior);
    assert_history_missing(&cassie, &session, &name);
}

#[test]
fn should_preserve_restart_when_discarding_history_in_implicit_transaction() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    session
        .begin_implicit_transaction()
        .expect("implicit begin");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE discard_restart_probe RESTART IDENTITY",
            vec![],
        )
        .expect("restart");
    // The restart is staged with the transaction, not committed natively.
    assert_eq!(cursor(&cassie, &name), prior);

    // Act
    session.discard_sequence_values();
    session.rollback_transaction();

    // Assert
    assert_eq!(cursor(&cassie, &name), prior);
    assert_history_missing(&cassie, &session, &name);
}

#[test]
fn should_make_progress_when_lastval_and_discard_share_sequence_state() {
    const CASE: &str = "discard-lastval-lock-order";
    const TEST: &str = "app::session_sequences::discard_restart_tests::should_make_progress_when_lastval_and_discard_share_sequence_state";
    if !crate::app::query_failure_watchdog::enter_child(TEST, CASE) {
        return;
    }
    {
        // Arrange
        let (cassie, session, _, _) = fixture();
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let last_session = session.clone();
        let last = std::thread::spawn(move || {
            last_session.last_sequence_value_observed(|| {
                held_tx.send(()).expect("sequence lock observed");
                release_rx.recv().expect("release lastval");
            })
        });
        held_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("lastval holds sequence lock");
        let (contended_tx, contended_rx) = mpsc::channel();
        let discard_session = session.clone();

        // Act
        let discard = std::thread::spawn(move || {
            discard_session.discard_sequence_values_observed(
                || contended_tx.send(()).expect("discard contention observed"),
                |_| {},
            );
        });
        contended_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("discard attempts sequence lock");
        release_tx.send(()).expect("release lastval");
        let value = last.join().expect("lastval worker").expect("last value");
        discard.join().expect("discard worker");

        // Assert
        assert_eq!(value, 40);
        assert_eq!(
            session
                .last_sequence_value()
                .expect_err("discarded history")
                .descriptor()
                .sql_state,
            "55000"
        );
        drop(cassie);
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_restore_identity_restart_after_discard_sequences_rollback() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    assert_history_is_40(&cassie, &session, &name);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE discard_restart_probe RESTART IDENTITY",
            vec![],
        )
        .expect("restart identity");
    let during_transaction = cursor(&cassie, &name);
    assert_eq!(during_transaction, prior);

    // Act
    cassie
        .execute_sql(&session, "DISCARD SEQUENCES", vec![])
        .expect("discard history");
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");
    let restored = cursor(&cassie, &name);
    let observer = cassie.create_session("observer", None);
    let rows = cassie
        .execute_sql(
            &observer,
            "SELECT payload FROM discard_restart_probe",
            vec![],
        )
        .expect("independent row read");

    // Assert
    assert_eq!(restored, prior);
    assert_eq!(rows.rows(), &[vec![Value::String("retained".into())]]);
    assert_history_missing(&cassie, &session, &name);
}

#[test]
fn should_keep_identity_restart_after_discard_sequences_commit() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE discard_restart_probe RESTART IDENTITY",
            vec![],
        )
        .expect("restart identity");
    let during_transaction = cursor(&cassie, &name);
    assert_eq!(during_transaction, prior);

    // Act
    cassie
        .execute_sql(&session, "DISCARD SEQUENCES", vec![])
        .expect("discard history");
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit");
    let committed = cursor(&cassie, &name);
    let observer = cassie.create_session("observer", None);
    let rows = cassie
        .execute_sql(
            &observer,
            "SELECT payload FROM discard_restart_probe",
            vec![],
        )
        .expect("independent row read");

    // Assert
    assert_ne!(committed, prior);
    assert_eq!(rows.rows(), &[] as &[Vec<Value>]);
    assert_history_missing(&cassie, &session, &name);
}

#[test]
fn should_forget_sequence_history_without_rewinding_outside_transaction() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    assert_history_is_40(&cassie, &session, &name);

    // Act
    cassie
        .execute_sql(&session, "DISCARD SEQUENCES", vec![])
        .expect("discard history");
    let after = cursor(&cassie, &name);

    // Assert
    assert_eq!(after, prior);
    assert_history_missing(&cassie, &session, &name);
    let next = cassie
        .execute_sql(&session, &format!("SELECT nextval('{name}')"), vec![])
        .expect("next value");
    assert_eq!(next.rows(), &[vec![Value::Int64(41)]]);
}

#[test]
fn should_restore_identity_restart_after_normal_rollback() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE discard_restart_probe RESTART IDENTITY",
            vec![],
        )
        .expect("restart identity");
    let during_transaction = cursor(&cassie, &name);

    // Act
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");
    let restored = cursor(&cassie, &name);
    let observer = cassie.create_session("observer", None);
    let rows = cassie
        .execute_sql(
            &observer,
            "SELECT payload FROM discard_restart_probe",
            vec![],
        )
        .expect("independent row read");

    // Assert
    assert_eq!(during_transaction, prior);
    assert_eq!(restored, prior);
    assert_eq!(rows.rows(), &[vec![Value::String("retained".into())]]);
    assert_history_is_40(&cassie, &session, &name);
}

#[test]
fn should_reject_discard_all_without_losing_restart_restoration() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE discard_restart_probe RESTART IDENTITY",
            vec![],
        )
        .expect("restart identity");
    let during_transaction = cursor(&cassie, &name);

    // Act
    let result = cassie.execute_sql(&session, "DISCARD ALL", vec![]);
    let failed = session.is_transaction_failed();
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback failed block");
    let restored = cursor(&cassie, &name);
    let observer = cassie.create_session("observer", None);
    let rows = cassie
        .execute_sql(
            &observer,
            "SELECT payload FROM discard_restart_probe",
            vec![],
        )
        .expect("independent row read");

    // Assert
    let error: CassieError = result.expect_err("DISCARD ALL rejected in transaction");
    assert_eq!(error.descriptor().sql_state, "25001");
    assert!(failed);
    assert_eq!(during_transaction, prior);
    assert_eq!(restored, prior);
    assert_eq!(rows.rows(), &[vec![Value::String("retained".into())]]);
    assert_history_is_40(&cassie, &session, &name);
}

#[test]
fn should_forget_sequence_history_after_discard_all_outside_transaction() {
    // Arrange
    let (cassie, session, name, prior) = fixture();
    assert_history_is_40(&cassie, &session, &name);

    // Act
    cassie
        .execute_sql(&session, "DISCARD ALL", vec![])
        .expect("discard all");
    let after = cursor(&cassie, &name);

    // Assert
    assert_eq!(after, prior);
    assert_history_missing(&cassie, &session, &name);
    let next = cassie
        .execute_sql(&session, &format!("SELECT nextval('{name}')"), vec![])
        .expect("next value");
    assert_eq!(next.rows(), &[vec![Value::Int64(41)]]);
}
