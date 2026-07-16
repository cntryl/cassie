use cassie::app::Cassie;
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

#[test]
fn should_stage_writes_across_collections() {
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
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect("stage second collection");
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("commit");

        // Assert
        assert_eq!(session.transaction_status(), "idle");
    });
}

#[test]
fn should_stage_delete_across_collections() {
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
        cassie
            .execute_sql(
                session,
                "DELETE FROM transaction_stage_b WHERE title = 'beta'",
                vec![],
            )
            .expect("stage second collection delete");
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("commit");

        // Assert
        let rows = cassie
            .execute_sql(session, "SELECT id FROM transaction_stage_b", vec![])
            .expect("read deleted collection");
        assert!(rows.rows.is_empty());
    });
}

#[test]
fn should_discard_multi_collection_staged_rows_after_rollback() {
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
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect("stage second collection");

        // Act
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback transaction");
        let first_rows = cassie
            .execute_sql(session, "SELECT id FROM transaction_stage_a", vec![])
            .expect("read first collection");

        // Assert
        assert!(first_rows.rows.is_empty());
        assert_eq!(session.transaction_status(), "idle");
    });
}

#[test]
fn should_recover_after_multi_collection_rollback() {
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
        cassie
            .execute_sql(
                session,
                "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .expect("stage second collection");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback transaction");

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
fn should_commit_cross_collection_delete_cascade() {
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
    cassie
        .execute_sql(
            &session,
            "DELETE FROM transaction_cascade_parent WHERE title = 'alpha'",
            vec![],
        )
        .expect("stage cross-collection cascade");
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit cascade");

    // Assert
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
    assert!(parent.rows.is_empty());
    assert!(child.rows.is_empty());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_commit_cross_collection_update_cascade() {
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
    cassie
        .execute_sql(
            &session,
            "UPDATE transaction_update_parent SET code = 'two' WHERE title = 'alpha'",
            vec![],
        )
        .expect("stage cross-collection update cascade");
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit update cascade");

    // Assert
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
    assert_eq!(parent.rows, vec![vec![Value::String("two".to_string())]]);
    assert_eq!(child.rows, vec![vec![Value::String("two".to_string())]]);

    let _ = std::fs::remove_dir_all(path);
}
