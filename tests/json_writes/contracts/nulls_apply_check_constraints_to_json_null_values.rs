#[test]
fn should_apply_check_constraints_to_json_null_values() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_check_constraint");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_null_check_values (id TEXT PRIMARY KEY, doc JSON CHECK (doc > 0))",
                vec![],
            )
            .expect("create checked JSON table");

        // Act
        let sql_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_check_values (id, doc) VALUES ('sql-null', NULL)",
            vec![],
        );
        let json_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_check_values (id, doc) VALUES ('json-null', 'null')",
            vec![],
        );

        // Assert
        assert!(sql_null.is_ok());
        assert!(json_null.is_err());

        drop(session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_type_json_null_inside_unique_expression_indexes() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_expression_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_null_expression_values (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX json_null_expression_values_idx ON json_null_expression_values ((doc IS NULL))",
                vec![],
            )
            .expect("create unique expression index");

        // Act
        let json_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_expression_values (id, doc) VALUES ('json-null', 'null')",
            vec![],
        );
        let sql_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_expression_values (id, doc) VALUES ('sql-null', NULL)",
            vec![],
        );
        let duplicate_json_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_expression_values (id, doc) VALUES ('duplicate-json-null', 'null')",
            vec![],
        );

        // Assert
        assert!(json_null.is_ok());
        assert!(sql_null.is_ok());
        assert!(duplicate_json_null.is_err());

        drop(session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    });
}
