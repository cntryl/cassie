//! Known failed publications do not retarget an RR conflict intent.

use super::*;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

#[test]
fn should_wait_for_a_committing_read_committed_row_owner_before_reapplying() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("memory fixture");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE read_committed_wait (id INT PRIMARY KEY, a INT, b INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO read_committed_wait VALUES (1, 0, 0)",
            Vec::new(),
        )
        .expect("seed row");
    let first = cassie.create_session("first", None);
    let second = cassie.create_session("second", None);
    for session in [&first, &second] {
        cassie
            .execute_sql(session, "BEGIN", Vec::new())
            .expect("begin READ COMMITTED transaction");
    }
    cassie
        .execute_sql(
            &first,
            "UPDATE read_committed_wait SET a = 10 WHERE id = 1",
            Vec::new(),
        )
        .expect("first update owns row");
    let (wait_reached, wait_reached_rx) = std::sync::mpsc::channel();
    let _wait_observer = crate::app::install_row_wait_observer(wait_reached);
    let worker_cassie = cassie.clone();

    // Act
    let worker = std::thread::spawn(move || {
        worker_cassie
            .execute_sql(
                &second,
                "UPDATE read_committed_wait SET b = 20 WHERE id = 1",
                Vec::new(),
            )
            .expect("second update after owner commit");
        worker_cassie
            .execute_sql(&second, "COMMIT", Vec::new())
            .expect("second commit");
    });
    wait_reached_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("second update reached the row wait");

    // Assert
    assert!(!worker.is_finished(), "waiter completed before owner commit");
    cassie
        .execute_sql(&first, "COMMIT", Vec::new())
        .expect("first commit releases row owner");
    worker.join().expect("second update worker");
    let result = cassie
        .execute_sql(
            &setup,
            "SELECT id, a, b FROM read_committed_wait",
            Vec::new(),
        )
        .expect("final row");
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)]]
    );
}

#[test]
fn should_preserve_concurrent_read_committed_column_updates() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("memory fixture");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE read_committed_lost_update (id INT PRIMARY KEY, a INT, b INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO read_committed_lost_update VALUES (1, 0, 0)",
            Vec::new(),
        )
        .expect("seed row");
    let first = cassie.create_session("first", None);
    let second = cassie.create_session("second", None);
    for session in [&first, &second] {
        cassie
            .execute_sql(session, "BEGIN", Vec::new())
            .expect("begin READ COMMITTED transaction");
    }
    cassie
        .execute_sql(
            &first,
            "UPDATE read_committed_lost_update SET a = 10 WHERE id = 1",
            Vec::new(),
        )
        .expect("stage first update");
    let (wait_reached, wait_reached_rx) = std::sync::mpsc::channel();
    let _wait_observer = crate::app::install_row_wait_observer(wait_reached);
    let worker_cassie = cassie.clone();

    // Act
    let worker = std::thread::spawn(move || {
        worker_cassie
            .execute_sql(
                &second,
                "UPDATE read_committed_lost_update SET b = 20 WHERE id = 1",
                Vec::new(),
            )
            .expect("second update after owner commit");
        worker_cassie
            .execute_sql(&second, "COMMIT", Vec::new())
            .expect("second commit");
    });
    wait_reached_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("second update reached the row wait");
    assert!(!worker.is_finished(), "waiter completed before owner commit");
    cassie
        .execute_sql(&first, "COMMIT", Vec::new())
        .expect("first commit");
    worker.join().expect("second update worker");
    let result = cassie
        .execute_sql(
            &setup,
            "SELECT id, a, b FROM read_committed_lost_update",
            Vec::new(),
        )
        .expect("read committed row");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)]]
    );
}

#[test]
fn should_apply_overlapping_read_committed_increments_twice() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("memory fixture");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE read_committed_increment (id INT PRIMARY KEY, a INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO read_committed_increment VALUES (1, 0)",
            Vec::new(),
        )
        .expect("seed row");
    let first = cassie.create_session("first", None);
    let second = cassie.create_session("second", None);
    for session in [&first, &second] {
        cassie
            .execute_sql(session, "BEGIN", Vec::new())
            .expect("begin READ COMMITTED transaction");
    }
    cassie
        .execute_sql(
            &first,
            "UPDATE read_committed_increment SET a = a + 1 WHERE id = 1",
            Vec::new(),
        )
        .expect("stage first increment");
    let (wait_reached, wait_reached_rx) = std::sync::mpsc::channel();
    let _wait_observer = crate::app::install_row_wait_observer(wait_reached);
    let worker_cassie = cassie.clone();

    // Act
    let worker = std::thread::spawn(move || {
        worker_cassie
            .execute_sql(
                &second,
                "UPDATE read_committed_increment SET a = a + 1 WHERE id = 1",
                Vec::new(),
            )
            .expect("second increment after owner commit");
        worker_cassie
            .execute_sql(&second, "COMMIT", Vec::new())
            .expect("second commit");
    });
    wait_reached_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("second increment reached the row wait");
    assert!(!worker.is_finished(), "waiter completed before owner commit");
    cassie
        .execute_sql(&first, "COMMIT", Vec::new())
        .expect("first commit");
    worker.join().expect("second increment worker");
    let result = cassie
        .execute_sql(
            &setup,
            "SELECT a FROM read_committed_increment",
            Vec::new(),
        )
        .expect("read increment");

    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(2)]]);
}

#[test]
fn should_keep_repeatable_read_serialization_failure_for_row_contention() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("memory fixture");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(&setup, "CREATE TABLE rr_row_guard (id INT PRIMARY KEY, a INT, b INT)", Vec::new())
        .expect("table");
    cassie
        .execute_sql(&setup, "INSERT INTO rr_row_guard VALUES (1, 0, 0)", Vec::new())
        .expect("seed row");
    let first = cassie.create_session("first", None);
    let second = cassie.create_session("second", None);
    cassie
        .execute_sql(&first, "BEGIN ISOLATION LEVEL REPEATABLE READ", Vec::new())
        .expect("first begin");
    cassie
        .execute_sql(&second, "BEGIN ISOLATION LEVEL REPEATABLE READ", Vec::new())
        .expect("second begin");
    cassie
        .execute_sql(&first, "UPDATE rr_row_guard SET a = 10 WHERE id = 1", Vec::new())
        .expect("first update");
    let (updated, updated_rx) = std::sync::mpsc::channel();
    let worker_cassie = cassie.clone();

    // Act
    let worker = std::thread::spawn(move || {
        let update = worker_cassie.execute_sql(
            &second,
            "UPDATE rr_row_guard SET b = 20 WHERE id = 1",
            Vec::new(),
        );
        updated.send(update.is_ok()).expect("report update");
        worker_cassie.execute_sql(&second, "COMMIT", Vec::new())
    });
    assert!(updated_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("second update result"));
    cassie
        .execute_sql(&first, "COMMIT", Vec::new())
        .expect("first commit");
    let second_commit = worker.join().expect("second worker");

    // Assert
    assert_eq!(second_commit.expect_err("stale RR commit").descriptor().sql_state, "40001");
}

#[test]
fn should_preserve_autocommit_update_serialization() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("memory fixture");
    let session = cassie.create_session("autocommit", None);
    cassie
        .execute_sql(&session, "CREATE TABLE autocommit_row_guard (id INT PRIMARY KEY, a INT)", Vec::new())
        .expect("table");
    cassie
        .execute_sql(&session, "INSERT INTO autocommit_row_guard VALUES (1, 0)", Vec::new())
        .expect("seed row");

    // Act
    cassie
        .execute_sql(&session, "UPDATE autocommit_row_guard SET a = a + 1 WHERE id = 1", Vec::new())
        .expect("first autocommit update");
    cassie
        .execute_sql(&session, "UPDATE autocommit_row_guard SET a = a + 1 WHERE id = 1", Vec::new())
        .expect("second autocommit update");
    let result = cassie
        .execute_sql(&session, "SELECT a FROM autocommit_row_guard", Vec::new())
        .expect("read row");

    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(2)]]);
}

#[test]
fn should_reject_a_late_repeatable_conflict_without_changing_its_target() {
    // Arrange
    for action in [
        "DO NOTHING",
        "DO UPDATE SET v = excluded.v WHERE FALSE",
        "DO UPDATE SET v = excluded.v",
    ] {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE late_rr_conflicts (id INT PRIMARY KEY, k TEXT UNIQUE, v INT)",
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            "SELECT 1",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("old empty view");
        }
        let proposal = cassie.execute_sql(&session, &format!("INSERT INTO late_rr_conflicts VALUES (1, 'key', 9) ON CONFLICT (k) {action} RETURNING id"), Vec::new()).expect("initially uncontested proposal");
        cassie
            .midge
            .put_document(
                "late_rr_conflicts",
                Some("native-winner".to_owned()),
                serde_json::json!({"id":2,"k":"key","v":2}),
            )
            .expect("competing direct document commit");

        // Act
        let committed = cassie.execute_sql(&session, "COMMIT", Vec::new());
        let visible = cassie
            .execute_sql(
                &session,
                "SELECT id, v FROM late_rr_conflicts ORDER BY id",
                Vec::new(),
            )
            .expect("winner remains");

        // Assert
        assert_eq!(proposal.rows(), &[vec![Value::Int64(1)]]);
        assert_eq!(
            committed
                .expect_err("no RR retarget/retry")
                .descriptor()
                .sql_state,
            "40001"
        );
        assert_eq!(session.transaction_status(), "idle");
        assert_eq!(visible.rows(), &[vec![Value::Int64(2), Value::Int64(2)]]);
    }
}

#[test]
fn should_preserve_a_nonarbiter_unique_violation_during_repeatable_commit() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    let session = cassie.create_session("tester", None);
    for sql in ["CREATE TABLE other_rr_conflicts (id INT PRIMARY KEY, k TEXT UNIQUE, other_key TEXT UNIQUE, v INT)", "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE", "SELECT 1", "INSERT INTO other_rr_conflicts VALUES (1, 'target', 'collision', 9) ON CONFLICT (k) DO UPDATE SET v = excluded.v RETURNING id"] {
        cassie.execute_sql(&session, sql, Vec::new()).expect("uncontested arbiter");
    }
    cassie
        .midge
        .put_document(
            "other_rr_conflicts",
            Some("native-winner".to_owned()),
            serde_json::json!({"id":2,"k":"different","other_key":"collision","v":2}),
        )
        .expect("different unique constraint winner");

    // Act
    let committed = cassie.execute_sql(&session, "COMMIT", Vec::new());
    let visible = cassie
        .execute_sql(
            &session,
            "SELECT id, v FROM other_rr_conflicts ORDER BY id",
            Vec::new(),
        )
        .expect("winner remains");

    // Assert
    assert_eq!(
        committed
            .expect_err("nonarbiter violation remains native constraint failure")
            .descriptor()
            .sql_state,
        "23505"
    );
    assert_eq!(session.transaction_status(), "idle");
    assert_eq!(visible.rows(), &[vec![Value::Int64(2), Value::Int64(2)]]);
}

#[test]
fn should_reject_a_late_document_writer_using_a_new_rows_physical_id() {
    // Arrange
    for storage in ["", " WITH (storage = column_store)"] {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE same_id_rr_conflicts (id INT PRIMARY KEY, v INT){storage}"),
                Vec::new(),
            )
            .expect("table");
        for sql in [
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            "SELECT 1",
            "INSERT INTO same_id_rr_conflicts VALUES (1, 9)",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("new own row against empty view");
        }
        let physical = session
            .transaction_writes()
            .into_values()
            .flat_map(std::collections::BTreeMap::into_keys)
            .next()
            .expect("one actual staged physical id");
        cassie
            .midge
            .put_document(
                "same_id_rr_conflicts",
                Some(physical),
                serde_json::json!({"id":1,"v":2}),
            )
            .expect("same physical id direct publisher");

        // Act
        let committed = cassie.execute_sql(&session, "COMMIT", Vec::new());
        let visible = cassie
            .execute_sql(
                &session,
                "SELECT id, v FROM same_id_rr_conflicts",
                Vec::new(),
            )
            .expect("direct winner remains");

        // Assert
        assert_eq!(
            committed
                .expect_err("new own row cannot overwrite a newer direct publisher")
                .descriptor()
                .sql_state,
            "40001"
        );
        assert_eq!(session.transaction_status(), "idle");
        assert_eq!(visible.rows(), &[vec![Value::Int64(1), Value::Int64(2)]]);
    }
}
