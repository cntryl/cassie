#[test]
fn should_query_rows_after_creating_ordered_bounded_index() {
    // Arrange
    let path = data_dir("ordered_bounded_index_query");
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
                "CREATE TABLE ordered_bounded_index_query (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX ordered_bounded_title_idx ON ordered_bounded_index_query USING btree (title)",
                vec![],
            )
            .unwrap();
        for (title, body) in [("delta", "d"), ("alpha", "a"), ("charlie", "c"), ("beta", "b")] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO ordered_bounded_index_query (title, body) VALUES ($1, $2)",
                    vec![Value::String(title.to_string()), Value::String(body.to_string())],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM ordered_bounded_index_query ORDER BY title ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM ordered_bounded_index_query ORDER BY title ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=ordered_bounded_scan"));
        assert!(plan.contains("top_k_mode=storage"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_include_null_sort_keys_in_indexed_order_by_limit() {
    // Arrange
    let path = data_dir("scalar_index_order_null_keys");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        create_indexed_and_plain_tables(
            &cassie,
            &session,
            ["order_null_keys_indexed", "order_null_keys_plain"],
            "x INT",
            "x",
            &[Value::Int64(5), Value::Null, Value::Int64(3)],
        );

        // Act
        let run = |table: &str, direction: &str| {
            cassie
                .execute_sql(
                    &session,
                    &format!("SELECT x FROM {table} ORDER BY x {direction} LIMIT 5"),
                    vec![],
                )
                .expect("ordered limit query")
                .rows()
                .to_vec()
        };
        let indexed_asc = run("order_null_keys_indexed", "ASC");
        let plain_asc = run("order_null_keys_plain", "ASC");
        let indexed_desc = run("order_null_keys_indexed", "DESC");
        let plain_desc = run("order_null_keys_plain", "DESC");

        // Assert
        assert_eq!(
            indexed_asc,
            vec![
                vec![Value::Int64(3)],
                vec![Value::Int64(5)],
                vec![Value::Null],
            ]
        );
        assert_eq!(indexed_asc, plain_asc);
        assert_eq!(
            indexed_desc,
            vec![
                vec![Value::Null],
                vec![Value::Int64(5)],
                vec![Value::Int64(3)],
            ]
        );
        assert_eq!(indexed_desc, plain_desc);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_ordered_bounded_scan_for_not_null_indexed_column() {
    // Arrange
    let path = data_dir("scalar_index_order_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        create_indexed_and_plain_tables(
            &cassie,
            &session,
            ["order_not_null_indexed", "order_not_null_plain"],
            "x INT NOT NULL",
            "x",
            &[Value::Int64(5), Value::Int64(3), Value::Int64(9)],
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT x FROM order_not_null_indexed ORDER BY x DESC LIMIT 2",
                vec![],
            )
            .expect("ordered limit query");
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT x FROM order_not_null_indexed ORDER BY x DESC LIMIT 2",
                vec![],
            )
            .expect("explain ordered limit query");

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::Int64(9)], vec![Value::Int64(5)]]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=ordered_bounded_scan"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_replan_ordered_limit_after_dropping_not_null_on_indexed_column() {
    // Arrange
    let path = data_dir("scalar_index_order_drop_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        create_indexed_and_plain_tables(
            &cassie,
            &session,
            ["order_drop_not_null_indexed", "order_drop_not_null_plain"],
            "x INT NOT NULL",
            "x",
            &[Value::Int64(5), Value::Int64(9)],
        );
        let sql = "SELECT x FROM order_drop_not_null_indexed ORDER BY x DESC LIMIT 2";
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("warm ordered limit plan");
        for statement in [
            "ALTER TABLE order_drop_not_null_indexed ALTER COLUMN x DROP NOT NULL",
            "INSERT INTO order_drop_not_null_indexed (x) VALUES (NULL)",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect(statement);
        }

        // Act
        let result = cassie
            .execute_sql(&session, sql, vec![])
            .expect("ordered limit query after dropping NOT NULL");

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::Null], vec![Value::Int64(9)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
