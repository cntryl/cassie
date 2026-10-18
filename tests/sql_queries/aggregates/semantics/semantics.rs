#[test]
fn should_execute_grouped_count_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("aggregate_count");
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
                "CREATE TABLE aggregate_count_docs (category TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('b')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM aggregate_count_docs GROUP BY category ORDER BY category",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![
                vec![Value::String("a".to_string()), Value::Int64(2)],
                vec![Value::String("b".to_string()), Value::Int64(1)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_basic_numeric_aggregates_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("aggregate_numeric");
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
                "CREATE TABLE aggregate_numeric_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (5)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_numeric_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![
                Value::Int64(15),
                // avg(integer) is numeric in PostgreSQL 18.6.
                Value::String("5.0000000000000000".to_owned()),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_distinct_filter_to_grouped_aggregates() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
            "aggregate_local_distinct_filter",
            &[
                "CREATE TABLE aggregate_local_options (team TEXT, sku TEXT, amount INT, enabled BOOLEAN)",
                "INSERT INTO aggregate_local_options VALUES ('north','x',2,true),('north','x',3,false),('north','y',3,true),('south','z',5,true),('south','z',5,true)",
            ],
        );

    // Act
    let result = fixture.execute(
            "SELECT team, COUNT(DISTINCT sku) FILTER (WHERE enabled), SUM(DISTINCT amount) FILTER (WHERE enabled) FROM aggregate_local_options GROUP BY team ORDER BY team",
        );

    // Assert
    assert_eq!(
        result.expect("aggregate-local DISTINCT and FILTER").rows(),
        &[
            vec![
                Value::String("north".to_owned()),
                Value::Int64(2),
                Value::Int64(5),
            ],
            vec![
                Value::String("south".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
            ],
        ]
    );
}

#[test]
fn should_apply_order_by_inside_string_aggregate() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
            "aggregate_local_order_by",
            &[
                "CREATE TABLE aggregate_local_order (team TEXT, sku TEXT, amount INT)",
                "INSERT INTO aggregate_local_order VALUES ('north','x',2),('north','z',3),('north','y',3),('south','b',9),('south','a',4)",
            ],
        );

    // Act
    let result = fixture.execute(
            "SELECT team, STRING_AGG(sku, ',' ORDER BY amount DESC, sku) FROM aggregate_local_order GROUP BY team ORDER BY team",
        );

    // Assert
    assert_eq!(
        result.expect("ordered string aggregate").rows(),
        &[
            vec![
                Value::String("north".to_owned()),
                Value::String("y,z,x".to_owned()),
            ],
            vec![
                Value::String("south".to_owned()),
                Value::String("b,a".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_emit_grouping_set_detail_subtotal_grand_total_rows() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
            "aggregate_grouping_sets",
            &[
                "CREATE TABLE aggregate_grouping_values (region TEXT, product TEXT, amount INT)",
                "INSERT INTO aggregate_grouping_values VALUES ('east','book',2),('east','pen',3),('west','book',5)",
            ],
        );

    // Act
    let result = fixture.execute(
            "SELECT region, product, SUM(amount), GROUPING(region, product) FROM aggregate_grouping_values GROUP BY GROUPING SETS ((region, product), (region), ()) ORDER BY GROUPING(region, product), region NULLS FIRST, product NULLS FIRST",
        );

    // Assert
    assert_eq!(
        result.expect("grouping set rows and indicators").rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(2),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![Value::Null, Value::Null, Value::Int64(10), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_return_statistical_aggregate_values_for_float_inputs() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
            "aggregate_boolean_statistics",
            &[
                "CREATE TABLE aggregate_statistical_inputs (value FLOAT, keep BOOLEAN)",
                "INSERT INTO aggregate_statistical_inputs VALUES (1.0, true), (2.0, true), (3.0, false), (NULL, true)",
            ],
        );

    // Act
    let result = fixture.execute(
            "SELECT VAR_POP(value), VAR_SAMP(value), STDDEV_POP(value), STDDEV_SAMP(value), CORR(value, value), COVAR_POP(value, value), REGR_SLOPE(value, value) FROM aggregate_statistical_inputs",
        );

    // Assert
    let rows = result.expect("statistical aggregates over table inputs");
    assert_eq!(
        rows.rows(),
        &[vec![
            Value::Float64(0.666_666_666_666_666_6),
            Value::Float64(1.0),
            Value::Float64(0.816_496_580_927_726),
            Value::Float64(1.0),
            Value::Float64(1.0),
            Value::Float64(0.666_666_666_666_666_6),
            Value::Float64(1.0),
        ]]
    );
    assert_eq!(
        rows.columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701, 701, 701, 701, 701, 701]
    );
}

#[test]
fn should_execute_ordered_set_rank_aggregate_family() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_ordered_hypothetical",
        &[
            "CREATE TABLE aggregate_ordered_inputs (x INT)",
            "INSERT INTO aggregate_ordered_inputs VALUES (1), (3), (3), (10)",
        ],
    );

    // Act
    let result = fixture.execute(
            "SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x::FLOAT8) = 3.0, PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY x) = 3, MODE() WITHIN GROUP (ORDER BY x) = 3, RANK(3) WITHIN GROUP (ORDER BY x) = 2, DENSE_RANK(3) WITHIN GROUP (ORDER BY x) = 2 FROM aggregate_ordered_inputs",
        );

    // Assert
    assert_eq!(
        result
            .expect("ordered-set and hypothetical aggregates")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_execute_row_number_window_function_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("window_row_number");
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
                "CREATE TABLE window_scores (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'first', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'second', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('b', 'third', 30)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, title, row_number() OVER (PARTITION BY category ORDER BY score DESC) AS rank FROM window_scores ORDER BY category ASC, rank ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![
                vec![
                    Value::String("a".to_string()),
                    Value::String("second".to_string()),
                    Value::Int64(1)
                ],
                vec![
                    Value::String("a".to_string()),
                    Value::String("first".to_string()),
                    Value::Int64(2)
                ],
                vec![
                    Value::String("b".to_string()),
                    Value::String("third".to_string()),
                    Value::Int64(1)
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_basic_value_window_functions_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("window_basic_values");
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
                "CREATE TABLE window_values (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'alpha', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'beta', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'gamma', 20)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS rnk, dense_rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS dense, lag(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS prev, lead(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS next, first_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS first, last_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS last FROM window_values ORDER BY rnk ASC, title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![
                vec![
                    Value::String("alpha".to_string()),
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::Null,
                    Value::String("beta".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("alpha".to_string())
                ],
                vec![
                    Value::String("beta".to_string()),
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("beta".to_string())
                ],
                vec![
                    Value::String("gamma".to_string()),
                    Value::Int64(3),
                    Value::Int64(3),
                    Value::String("beta".to_string()),
                    Value::Null,
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_grouped_rows_with_having() {
    // Arrange
    use_local_storage();
    let path = data_dir("aggregate_having");
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
                "CREATE TABLE aggregate_having_sales (category TEXT, amount INT)",
                vec![],
            )

.unwrap();
        for sql in [
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 7)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 5)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('b', 3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, SUM(amount) AS total FROM aggregate_having_sales GROUP BY category HAVING SUM(amount) > 10",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("a".to_string()), Value::Int64(12)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_each_unaliased_aggregate_sharing_a_function_name() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_same_name_outputs",
        &[
            "CREATE TABLE t (k INT, v INT)",
            "INSERT INTO t (k, v) VALUES (1, 10)",
            "INSERT INTO t (k, v) VALUES (2, NULL)",
            "INSERT INTO t (k, v) VALUES (NULL, 30)",
        ],
    );

    // Act
    let counts = fixture.rows("SELECT COUNT(*), COUNT(v) FROM t");
    let reversed = fixture.rows("SELECT COUNT(v), COUNT(*) FROM t");
    let sums = fixture.rows("SELECT SUM(k), SUM(v) FROM t");
    let grouped = fixture.rows("SELECT k, COUNT(*), COUNT(v) FROM t WHERE k = 2 GROUP BY k");

    // Assert
    assert_eq!(counts, vec![vec![Value::Int64(3), Value::Int64(2)]]);
    assert_eq!(reversed, vec![vec![Value::Int64(2), Value::Int64(3)]]);
    assert_eq!(sums, vec![vec![Value::Int64(3), Value::Int64(40)]]);
    assert_eq!(
        grouped,
        vec![vec![Value::Int64(2), Value::Int64(1), Value::Int64(0)]]
    );
}

#[test]
fn should_report_each_unaliased_aggregate_from_column_batch_summaries() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_same_name_column_batch",
        &[
            "CREATE TABLE tc (k INT, v INT)",
            "INSERT INTO tc (k, v) VALUES (1, 10)",
            "INSERT INTO tc (k, v) VALUES (2, NULL)",
            "INSERT INTO tc (k, v) VALUES (NULL, 30)",
            "CREATE INDEX tc_idx ON tc USING column (k, v) WITH (segment_size = 2)",
        ],
    );

    // Act
    let unfiltered = fixture.rows("SELECT COUNT(*), COUNT(v), SUM(k), SUM(v) FROM tc");
    let filtered = fixture.rows("SELECT COUNT(k), COUNT(v) FROM tc WHERE k > 0");

    // Assert
    assert_eq!(
        unfiltered,
        vec![vec![
            Value::Int64(3),
            Value::Int64(2),
            Value::Int64(3),
            Value::Int64(40)
        ]]
    );
    assert_eq!(filtered, vec![vec![Value::Int64(2), Value::Int64(1)]]);
}

#[test]
fn should_report_aggregate_value_when_a_column_shares_its_name() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_column_name_collision",
        &[
            "CREATE TABLE t2 (sum INT, x INT)",
            "INSERT INTO t2 (sum, x) VALUES (7, 1)",
            "INSERT INTO t2 (sum, x) VALUES (7, 2)",
            "CREATE TABLE u2 (count INT)",
            "INSERT INTO u2 (count) VALUES (5)",
            "INSERT INTO u2 (count) VALUES (5)",
        ],
    );

    // Act
    let sums = fixture.rows("SELECT sum, sum(x) FROM t2 GROUP BY sum");
    let counts = fixture.rows("SELECT count, count(*) FROM u2 GROUP BY count");

    // Assert
    assert_eq!(sums, vec![vec![Value::Int64(7), Value::Int64(3)]]);
    assert_eq!(counts, vec![vec![Value::Int64(5), Value::Int64(2)]]);
}

#[test]
fn should_accept_implicit_group_projection_without_bare_columns() {
    // Arrange
    let fixture = grouping_fixture("aggregate_implicit_group_valid");

    // Act
    let rows = fixture.rows("SELECT COUNT(*) AS n, MAX(v) + 1 AS top, 'x' AS tag FROM m");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(4),
            Value::Int64(10),
            Value::String("x".to_string())
        ]]
    );
}

#[test]
fn should_order_grouped_rows_by_group_key_or_output_alias() {
    // Arrange
    let fixture = grouping_fixture("aggregate_order_by_grouped");

    // Act
    let by_key = fixture.rows("SELECT g, COUNT(*) AS n FROM m GROUP BY g ORDER BY g DESC");
    let by_alias = fixture.rows("SELECT g AS label FROM m GROUP BY g ORDER BY label");

    // Assert
    assert_eq!(
        by_key,
        vec![
            vec![Value::String("c".to_string()), Value::Int64(1)],
            vec![Value::String("b".to_string()), Value::Int64(1)],
            vec![Value::String("a".to_string()), Value::Int64(2)],
        ]
    );
    assert_eq!(
        by_alias,
        vec![
            vec![Value::String("a".to_string())],
            vec![Value::String("b".to_string())],
            vec![Value::String("c".to_string())],
        ]
    );
}

#[test]
fn should_order_grouped_rows_by_aggregate_expression() {
    // Arrange
    let fixture = ordered_groups_fixture("aggregate_order_by_aggregate");

    // Act
    let orders = [
        "SELECT g, SUM(v) AS s FROM t GROUP BY g ORDER BY SUM(v) DESC",
        "SELECT g, SUM(v) AS s FROM t GROUP BY g ORDER BY s DESC",
        "SELECT g, SUM(v) AS s FROM t GROUP BY g ORDER BY s",
        "SELECT g, COUNT(*) AS n FROM t GROUP BY g ORDER BY COUNT(*) DESC, g",
        "SELECT g FROM t GROUP BY g ORDER BY MAX(v)",
        "SELECT g, AVG(v) FROM t GROUP BY g ORDER BY AVG(v) DESC",
        "SELECT g FROM t GROUP BY g ORDER BY COUNT(*) DESC, g LIMIT 1",
        "SELECT g FROM t GROUP BY g ORDER BY SUM(v) + 1 DESC",
    ]
    .map(|sql| labels(&fixture.rows(sql)));

    // Assert
    assert_eq!(
        orders,
        [
            vec!["b", "c", "a"],
            vec!["b", "c", "a"],
            vec!["a", "c", "b"],
            vec!["a", "b", "c"],
            vec!["a", "c", "b"],
            vec!["b", "c", "a"],
            vec!["a"],
            vec!["b", "c", "a"],
        ]
        .map(|labels| labels.into_iter().map(String::from).collect::<Vec<_>>())
    );
}
