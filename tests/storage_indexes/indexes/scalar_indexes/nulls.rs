#[test]
fn should_include_null_trailing_keys_in_composite_index_range_scan() {
    // Arrange
    let path = data_dir("scalar_index_composite_null_trailing");
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
            [
                "composite_null_trailing_indexed",
                "composite_null_trailing_plain",
            ],
            "x INT, y INT",
            "x, y",
            &[Value::Int64(5), Value::Int64(7)],
        );

        // Act
        let run = |table: &str| {
            let mut rows = cassie
                .execute_sql(
                    &session,
                    &format!("SELECT x FROM {table} WHERE x > 1"),
                    vec![],
                )
                .expect("composite range query")
                .rows()
                .to_vec();
            rows.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
            rows
        };
        let indexed = run("composite_null_trailing_indexed");
        let plain = run("composite_null_trailing_plain");

        // Assert
        assert_eq!(indexed, vec![vec![Value::Int64(5)], vec![Value::Int64(7)]]);
        assert_eq!(indexed, plain);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_ordered_index_scan_for_nullable_leading_order_key() {
    // Arrange
    let path = data_dir("scalar_index_order_nullable_leading_key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE order_nullable_leading (status TEXT, score INT)",
                "CREATE INDEX order_nullable_leading_idx ON order_nullable_leading USING btree (status, score)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).expect(sql);
            }
            for (status, score) in [
                ("open", Value::Int64(5)),
                ("open", Value::Null),
                ("open", Value::Int64(3)),
                ("closed", Value::Int64(1)),
            ] {
                cassie
                    .execute_sql(
                        &session,
                        "INSERT INTO order_nullable_leading (status, score) VALUES ($1, $2)",
                        vec![Value::String(status.to_string()), score],
                    )
                    .expect("insert row");
            }
            let run = |sql: &str| {
                let rows = cassie
                    .execute_sql(&session, sql, vec![Value::String("open".to_string())])
                    .expect(sql)
                    .rows().to_vec();
                let Value::String(plan) = cassie
                    .execute_sql(
                        &session,
                        &format!("EXPLAIN {sql}"),
                        vec![Value::String("open".to_string())],
                    )
                    .expect("explain")
                    .rows()[0][0]
                    .clone()
                else {
                    panic!("expected textual plan");
                };
                (rows, plan)
            };

            // Act
            let (filled, filled_plan) = run(
                "SELECT score FROM order_nullable_leading WHERE status = $1 ORDER BY score LIMIT 2",
            );
            let (unfilled, unfilled_plan) = run(
                "SELECT score FROM order_nullable_leading WHERE status = $1 ORDER BY score LIMIT 3",
            );
            let (descending, descending_plan) = run(
                "SELECT score FROM order_nullable_leading WHERE status = $1 ORDER BY score DESC LIMIT 2",
            );

            // Assert
            assert_eq!(filled, vec![vec![Value::Int64(3)], vec![Value::Int64(5)]]);
            assert!(filled_plan.contains("access_path=ordered_bounded_scan"));
            assert_eq!(
                unfilled,
                vec![
                    vec![Value::Int64(3)],
                    vec![Value::Int64(5)],
                    vec![Value::Null],
                ]
            );
            assert!(unfilled_plan.contains("access_path=ordered_bounded_scan"));
            assert_eq!(descending, vec![vec![Value::Null], vec![Value::Int64(5)]]);
            assert!(descending_plan.contains("index=none"));

            let _ = std::fs::remove_dir_all(path);
        });
}
