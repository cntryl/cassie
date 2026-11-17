//! A detached restart retains its old overlay after its root owner retires.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::sync::mpsc;
use std::time::Duration;

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, Vec::new()).expect(sql);
}

#[test]
fn should_keep_a_detached_identity_restart_out_of_a_replacement_transaction() {
    for terminal in ["COMMIT", "ROLLBACK"] {
        // Arrange
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("restart_owner", None);
        let observer = cassie.create_session("restart_observer", None);
        for sql in [
            "CREATE SEQUENCE detached_restart_seq",
            "CREATE TABLE detached_restart_rows (id BIGINT DEFAULT nextval('detached_restart_seq'))",
            "ALTER SEQUENCE detached_restart_seq OWNED BY detached_restart_rows.id",
            "INSERT INTO detached_restart_rows DEFAULT VALUES",
            "INSERT INTO detached_restart_rows DEFAULT VALUES",
            "BEGIN",
        ] {
            execute(&cassie, &session, sql);
        }
        let metadata = cassie
            .catalog
            .list_sequences()
            .into_iter()
            .find(|sequence| crate::catalog::local_name(&sequence.name) == "detached_restart_seq")
            .expect("owned sequence");
        let original = cassie
            .midge
            .get_sequence(&metadata.name)
            .expect("native sequence")
            .expect("persisted sequence");
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let worker_cassie = cassie.clone();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            let _pause = Cassie::pause_truncate_restart_after_overlay_capture_for_test(
                entered_tx, resume_rx,
            );
            worker_cassie.execute_sql(
                &worker_session,
                "TRUNCATE detached_restart_rows RESTART IDENTITY",
                Vec::new(),
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("restart captured A overlay");

        // Act
        execute(&cassie, &session, "ROLLBACK");
        execute(&cassie, &session, "BEGIN");
        let replacement = session.transaction_unique_owner_id();
        resume_tx.send(()).expect("resume detached A");
        let stale = worker.join().expect("restart worker");
        let unchanged = cassie
            .midge
            .get_sequence(&metadata.name)
            .expect("native sequence")
            .expect("sequence retained");
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id FROM detached_restart_rows ORDER BY id",
                Vec::new(),
            )
            .expect("B remains usable");

        // Assert
        assert_eq!(
            stale
                .expect_err("retired restart must not publish")
                .sql_state(),
            "25000"
        );
        assert_eq!(session.transaction_status(), "in_transaction");
        assert_eq!(session.transaction_unique_owner_id(), replacement);
        assert_eq!(unchanged.object_id, original.object_id);
        assert_eq!(unchanged.cursor(), original.cursor());
        assert_eq!(rows.rows(), &[vec![Value::Int64(1)], vec![Value::Int64(2)]]);
        let published = cassie
            .catalog
            .get_sequence(&metadata.name)
            .expect("published sequence");
        assert_eq!(published.object_id, original.object_id);
        assert_eq!(published.cursor(), original.cursor());
        execute(&cassie, &session, terminal);
        let next = cassie
            .execute_sql(
                &observer,
                "SELECT nextval('detached_restart_seq')",
                Vec::new(),
            )
            .expect("next native value");
        assert_eq!(next.rows()[0][0], Value::Int64(3));
    }
}
