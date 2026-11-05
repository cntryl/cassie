//! Known failed publications do not retarget an RR conflict intent.

use super::*;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

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
