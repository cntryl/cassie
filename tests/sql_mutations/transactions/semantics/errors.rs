#[test]
fn should_reject_unsupported_transaction_control_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_unsupported");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let statement = cassie.execute_sql(
            &session,
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            vec![],
        );

        // Assert
        assert!(statement.is_err());
        assert!(statement.unwrap_err().to_string().contains("unsupported"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_savepoint_outside_transaction() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_savepoint_outside");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let savepoint = cassie.execute_sql(&session, "SAVEPOINT sp", vec![]);

        // Assert
        assert!(savepoint.is_err());
        assert!(savepoint
            .unwrap_err()
            .to_string()
            .contains("can only be used in transaction blocks"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_advisory_lock_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_advisory_lock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_advisory_lock (id INT)",
                vec![],
            )
            .unwrap();

        // Act
        let lock = cassie.execute_sql(
            &session,
            "SELECT pg_advisory_lock(1) FROM transaction_advisory_lock",
            vec![],
        );

        // Assert
        assert!(lock.is_err());
        assert!(lock
            .unwrap_err()
            .to_string()
            .contains("unsupported function"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_work_after_transaction_error_until_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_failed_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_failed_state (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        let failed_insert = cassie.execute_sql(
            &session,
            "INSERT INTO transaction_failed_state (title) VALUES (NULL)",
            vec![],
        );
        assert!(failed_insert.is_err());

        // Act
        let selected = cassie.execute_sql(
            &session,
            "SELECT title FROM transaction_failed_state",
            vec![],
        );

        // Assert
        assert!(selected.is_err());
        assert!(selected
            .unwrap_err()
            .to_string()
            .contains("current transaction is aborted"));

        let _ = std::fs::remove_dir_all(path);
    });
}
