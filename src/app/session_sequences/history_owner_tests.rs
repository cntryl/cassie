//! Independent sequence completion does not publish history into a replacement owner.
use super::*;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

thread_local! {
    static NEXT: RefCell<Option<(SyncSender<()>, Receiver<()>)>> = const { RefCell::new(None) };
}
pub(super) fn after_native() {
    if let Some((entered, resume)) = NEXT.with(|slot| slot.borrow_mut().take()) {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}

#[test]
fn should_preserve_b_history_after_an_admitted_a_sequence_effect_finishes() {
    for sql in [
        "SELECT nextval('history_sequence')",
        "SELECT setval('history_sequence', 42)",
        "INSERT INTO history_rows DEFAULT VALUES",
    ] {
        // Arrange
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("history_owner", None);
        for setup in [
            "CREATE SEQUENCE history_sequence",
            "CREATE SEQUENCE b_sequence",
            "CREATE TABLE history_rows (id BIGINT DEFAULT nextval('history_sequence'))",
            "BEGIN",
        ] {
            cassie.execute_sql(&session, setup, vec![]).unwrap();
        }
        let sequence = cassie
            .catalog
            .list_sequences()
            .into_iter()
            .find(|value| crate::catalog::local_name(&value.name) == "history_sequence")
            .unwrap();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let worker_cassie = cassie.clone();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            NEXT.with(|slot| assert!(slot.replace(Some((entered_tx, resume_rx))).is_none()));
            worker_cassie.execute_sql(&worker_session, sql, vec![])
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

        // Act
        session.rollback_transaction();
        session.begin_transaction(None).unwrap();
        cassie
            .execute_sql(&session, "DISCARD SEQUENCES", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "SELECT nextval('b_sequence')", vec![])
            .unwrap();
        let current = session.transaction_unique_owner_id();
        resume_tx.send(()).unwrap();
        let result = worker.join().unwrap();
        let native = cassie.midge.get_sequence(&sequence.name).unwrap().unwrap();
        let published = cassie.catalog.get_sequence(&sequence.name).unwrap();
        let last = cassie
            .execute_sql(&session, "SELECT lastval()", vec![])
            .unwrap();
        let currval =
            session.current_sequence_value(&sequence.name, sequence.object_id, "history_sequence");

        // Assert
        assert_eq!(result.unwrap_err().sql_state(), "25000");
        assert_eq!(native.cursor(), published.cursor());
        assert_ne!(native.cursor(), sequence.cursor());
        assert_eq!(last.rows()[0][0], Value::Int64(1));
        assert_eq!(currval.unwrap_err().sql_state(), "55000");
        assert_eq!(session.transaction_unique_owner_id(), current);
        assert!(!session.is_transaction_failed());
        let rows = cassie
            .execute_sql(&session, "SELECT id FROM history_rows", vec![])
            .unwrap();
        assert!(rows.rows().is_empty());
    }
}

#[test]
fn should_publish_sequence_history_for_its_current_owner() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("history_current", None);
    cassie
        .execute_sql(&session, "CREATE SEQUENCE current_history", vec![])
        .unwrap();
    session.begin_transaction(None).unwrap();

    // Act
    let value = cassie
        .execute_sql(&session, "SELECT nextval('current_history')", vec![])
        .unwrap();
    let last = cassie
        .execute_sql(&session, "SELECT lastval()", vec![])
        .unwrap();
    let assigned = cassie
        .execute_sql(&session, "SELECT setval('current_history', 42)", vec![])
        .unwrap();
    let current = cassie
        .execute_sql(&session, "SELECT currval('current_history')", vec![])
        .unwrap();

    // Assert
    assert_eq!(value.rows()[0][0], Value::Int64(1));
    assert_eq!(last.rows()[0][0], Value::Int64(1));
    assert_eq!(assigned.rows()[0][0], Value::Int64(42));
    assert_eq!(current.rows()[0][0], Value::Int64(42));
    assert!(!session.is_transaction_failed());
}
