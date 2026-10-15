#[test]
fn should_keep_autocommit_writes_visible_after_success() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_autocommit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_autocommit (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_autocommit (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(&session, "SELECT title FROM transaction_autocommit", vec![])
            .unwrap();

        // Assert
        assert_eq!(session.transaction_status(), "idle");
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_bump_data_epoch_for_no_op_delete() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_no_op_delete_data_epoch");
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
                "CREATE TABLE no_op_delete_epoch (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO no_op_delete_epoch (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        let before_epoch = cassie.midge.data_epoch().unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM no_op_delete_epoch WHERE id = 2",
                vec![],
            )
            .unwrap();
        let after_epoch = cassie.midge.data_epoch().unwrap();

        // Assert
        assert_eq!(deleted.command(), "DELETE 0");
        assert_eq!(before_epoch, after_epoch);
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM no_op_delete_epoch ORDER BY id",
                vec![],
            )
            .unwrap();
        assert_eq!(rows.rows(), vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_replace_same_id_in_single_statement() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_same_id_replace");
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
                "CREATE TABLE same_id_replace (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO same_id_replace (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let replaced = cassie
            .execute_sql(
                &session,
                "INSERT INTO same_id_replace (id, title) VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title",
                vec![],
            )
            .unwrap();

        // Assert
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM same_id_replace WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(replaced.command(), "INSERT 0 1");
        assert_eq!(rows.rows(), vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}
