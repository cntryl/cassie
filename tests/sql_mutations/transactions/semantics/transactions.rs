#[test]
fn should_transition_session_state_for_transaction_control() {
    // Arrange
    let path = data_dir("transaction_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let begin = cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        let during = session.transaction_status();
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let after = session.transaction_status();

        // Assert
        assert_eq!(begin.command(), "BEGIN");
        assert_eq!(during, "in_transaction");
        assert_eq!(commit.command(), "COMMIT");
        assert_eq!(after, "idle");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_restore_idle_state_on_rollback() {
    // Arrange
    let path = data_dir("transaction_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        let rollback = cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let after = session.transaction_status();

        // Assert
        assert_eq!(rollback.command(), "ROLLBACK");
        assert_eq!(after, "idle");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_restore_search_path_after_transaction_rollback() {
    // Arrange
    let path = data_dir("transaction_rollback_search_path");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        setup_search_path_rollback_tables(&cassie, &session);
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "SET application_name TO 'before_savepoint'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT before_setting_change", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SET application_name TO 'after_savepoint'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SET search_path TO other_schema", vec![])
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ROLLBACK TO SAVEPOINT before_setting_change",
                vec![],
            )
            .unwrap();
        let savepoint_path = cassie
            .execute_sql(&session, "SHOW search_path", vec![])
            .unwrap();
        let savepoint_application_name = cassie
            .execute_sql(&session, "SHOW application_name", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "SET search_path TO other_schema", vec![])
            .unwrap();

        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let search_path = cassie
            .execute_sql(&session, "SHOW search_path", vec![])
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT value FROM transaction_search_path",
                vec![],
            )
            .unwrap();
        let application_name = cassie
            .execute_sql(&session, "SHOW application_name", vec![])
            .unwrap();

        // Assert
        assert_eq!(
            savepoint_path.rows(),
            vec![vec![Value::String("public".to_string())]]
        );
        assert_eq!(
            savepoint_application_name.rows(),
            vec![vec![Value::String("before_savepoint".to_string())]]
        );
        assert_eq!(
            search_path.rows(),
            vec![vec![Value::String("public".to_string())]]
        );
        assert_eq!(
            application_name.rows(),
            vec![vec![Value::String(String::new())]]
        );
        assert_eq!(selected.rows(), vec![vec![Value::Int64(1)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rollback_to_savepoint_discard_later_writes() {
    // Arrange
    let path = data_dir("transaction_savepoint_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_savepoint_rollback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_rollback (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_savepoint_rollback ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_release_savepoint_prevent_later_rollback_to_it() {
    // Arrange
    let path = data_dir("transaction_savepoint_release");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .unwrap();

        // Act
        cassie
            .execute_sql(&session, "RELEASE SAVEPOINT sp", vec![])
            .unwrap();
        let rollback = cassie.execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![]);

        // Assert
        assert!(rollback.is_err());
        assert!(rollback.unwrap_err().to_string().contains("savepoint"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rollback_to_savepoint_recover_failed_transaction() {
    // Arrange
    let path = data_dir("transaction_savepoint_failed_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_savepoint_failed_recovery (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .unwrap();
        let failed_insert = cassie.execute_sql(
            &session,
            "INSERT INTO transaction_savepoint_failed_recovery (title) VALUES (NULL)",
            vec![],
        );
        assert!(failed_insert.is_err());

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .unwrap();
        let status = session.transaction_status();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_failed_recovery (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_savepoint_failed_recovery",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(status, "in_transaction");
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_writes_after_rollback() {
    // Arrange
    let path = data_dir("transaction_rollback_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_rollback_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_rollback_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_rollback_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_commit_multi_collection_transaction_atomically() {
    // Arrange
    let path = data_dir("transaction_multi_collection_atomic");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE tx_multi_a (id TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE tx_multi_b (email TEXT)", vec![])
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO tx_multi_a (id) VALUES ('row-1')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO tx_multi_b (email) VALUES ('alice@example.com')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();

        // Assert
        let collected_from_a = cassie
            .execute_sql(&session, "SELECT id FROM tx_multi_a", vec![])
            .unwrap()
            .rows()
            .to_vec();
        assert_eq!(collected_from_a.len(), 1);
        let collected_from_b = cassie
            .execute_sql(
                &session,
                "SELECT email FROM tx_multi_b ORDER BY email",
                vec![],
            )
            .unwrap()
            .rows()
            .to_vec();
        assert_eq!(
            collected_from_b,
            vec![vec![Value::String("alice@example.com".into())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_allow_work_after_failed_transaction_rollback() {
    // Arrange
    let path = data_dir("transaction_failed_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_failed_recovery (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        let failed_insert = cassie.execute_sql(
            &session,
            "INSERT INTO transaction_failed_recovery (title) VALUES (NULL)",
            vec![],
        );
        assert!(failed_insert.is_err());
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_failed_recovery",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_update_after_rollback() {
    // Arrange
    let path = data_dir("transaction_update_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_update_rollback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_rollback SET title = 'beta'",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_update_rollback",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_delete_after_rollback() {
    // Arrange
    let path = data_dir("transaction_delete_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_delete_rollback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_delete_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_delete_rollback WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_delete_rollback",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_update_before_commit() {
    // Arrange
    let path = data_dir("transaction_update_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_update_read_your_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_read_your_writes SET title = 'beta'",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_update_read_your_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_delete_before_commit() {
    // Arrange
    let path = data_dir("transaction_delete_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_delete_read_your_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_delete_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_delete_read_your_writes WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_delete_read_your_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
