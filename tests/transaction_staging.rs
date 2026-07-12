use cassie::app::{Cassie, CassieError};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn with_two_collections<T>(
    label: &str,
    test: impl FnOnce(&Cassie, &cassie::app::CassieSession) -> T,
) -> T {
    with_fallback();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_stage_a (id INT PRIMARY KEY, title TEXT)",
            vec![],
        )
        .expect("create first collection");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_stage_b (id INT PRIMARY KEY, title TEXT)",
            vec![],
        )
        .expect("create second collection");
    let result = test(&cassie, &session);
    let _ = std::fs::remove_dir_all(path);
    result
}

fn assert_multi_collection_error(error: CassieError) {
    assert!(matches!(
        error,
        CassieError::Unsupported(message)
            if message.contains("transactions may modify only one collection")
    ));
}

#[test]
fn should_reject_second_collection_while_staging_write() {
    // Arrange
    with_two_collections("transaction_stage_write", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .expect("stage first collection");

        // Act
        let error = cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect_err("second collection should fail before commit");

        // Assert
        assert_multi_collection_error(error);
        assert_eq!(session.transaction_status(), "failed");
    });
}

#[test]
fn should_reject_second_collection_while_staging_delete() {
    // Arrange
    with_two_collections("transaction_stage_delete", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect("seed second collection");
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .expect("stage first collection");

        // Act
        let error = cassie
            .execute_sql(
                session,
                "DELETE FROM transaction_stage_b WHERE title = 'beta'",
                vec![],
            )
            .expect_err("second collection delete should fail while staging");

        // Assert
        assert_multi_collection_error(error);
        assert_eq!(session.transaction_status(), "failed");
    });
}

#[test]
fn should_discard_staged_rows_after_rejected_collection_rollback() {
    // Arrange
    with_two_collections("transaction_stage_rollback", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .expect("stage first collection");
        let _ = cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect_err("second collection should fail");

        // Act
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback failed transaction");
        let first_rows = cassie
            .execute_sql(session, "SELECT id FROM transaction_stage_a", vec![])
            .expect("read first collection");

        // Assert
        assert!(first_rows.rows.is_empty());
        assert_eq!(session.transaction_status(), "idle");
    });
}

#[test]
fn should_recover_after_staging_limit_rollback() {
    // Arrange
    with_two_collections("transaction_stage_recovery", |cassie, session| {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .expect("stage first collection");
        let _ = cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect_err("second collection should fail");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback failed transaction");

        // Act
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin retry");
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect("stage collection after rollback");
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("commit retry");
        let rows = cassie
            .execute_sql(
                session,
                "SELECT title FROM transaction_stage_b WHERE id = 1",
                vec![],
            )
            .expect("read committed retry");

        // Assert
        assert_eq!(rows.rows, vec![vec![Value::String("beta".to_string())]]);
    });
}

#[test]
fn should_preflight_cross_collection_delete_cascade() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_stage_cascade");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_cascade_parent (id INT PRIMARY KEY, title TEXT)",
            vec![],
        )
        .expect("create parent");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_cascade_child (id INT PRIMARY KEY, parent_id INT, CONSTRAINT transaction_cascade_child_parent FOREIGN KEY (parent_id) REFERENCES transaction_cascade_parent(id) ON DELETE CASCADE)",
            vec![],
        )
        .expect("create child");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO transaction_cascade_parent (id, title) VALUES (1, 'alpha')",
            vec![],
        )
        .expect("seed parent");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO transaction_cascade_child (id, parent_id) VALUES (1, 1)",
            vec![],
        )
        .expect("seed child");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "DELETE FROM transaction_cascade_parent WHERE title = 'alpha'",
            vec![],
        )
        .expect_err("cross-collection cascade should fail before staging");

    // Assert
    assert_multi_collection_error(error);
    assert_eq!(session.transaction_status(), "failed");
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback cascade rejection");
    let parent = cassie
        .execute_sql(
            &session,
            "SELECT id FROM transaction_cascade_parent",
            vec![],
        )
        .expect("read parent after rollback");
    let child = cassie
        .execute_sql(&session, "SELECT id FROM transaction_cascade_child", vec![])
        .expect("read child after rollback");
    assert_eq!(parent.rows.len(), 1);
    assert_eq!(child.rows.len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preflight_cross_collection_update_cascade() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_stage_update_cascade");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_update_parent (id INT PRIMARY KEY, code TEXT UNIQUE, title TEXT)",
            vec![],
        )
        .expect("create parent");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_update_child (parent_code TEXT, title TEXT, CONSTRAINT transaction_update_child_parent FOREIGN KEY (parent_code) REFERENCES transaction_update_parent(code) ON UPDATE CASCADE)",
            vec![],
        )
        .expect("create child");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO transaction_update_parent (id, code, title) VALUES (1, 'one', 'alpha')",
            vec![],
        )
        .expect("seed parent");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO transaction_update_child (parent_code, title) VALUES ('one', 'child')",
            vec![],
        )
        .expect("seed child");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "UPDATE transaction_update_parent SET code = 'two' WHERE title = 'alpha'",
            vec![],
        )
        .expect_err("cross-collection update cascade should fail before staging");

    // Assert
    assert_multi_collection_error(error);
    assert_eq!(session.transaction_status(), "failed");
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback cascade rejection");
    let parent = cassie
        .execute_sql(
            &session,
            "SELECT code FROM transaction_update_parent",
            vec![],
        )
        .expect("read parent after rollback");
    let child = cassie
        .execute_sql(
            &session,
            "SELECT parent_code FROM transaction_update_child",
            vec![],
        )
        .expect("read child after rollback");
    assert_eq!(parent.rows, vec![vec![Value::String("one".to_string())]]);
    assert_eq!(child.rows, vec![vec![Value::String("one".to_string())]]);

    let _ = std::fs::remove_dir_all(path);
}
