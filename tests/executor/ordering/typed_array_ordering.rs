use super::support_array_order::seed_numeric_arrays;
use super::support_read_equivalence::{sql, with_fixture};
use cassie::types::Value;

#[test]
fn should_order_declared_arrays_elementwise() {
    // Arrange
    with_fixture("typed_array_order", |cassie, session| {
        seed_numeric_arrays(cassie, session);
        let json = sql(cassie, session, "SELECT item FROM json_values ORDER BY arr");
        assert_eq!(
            json.rows(),
            ["d", "b", "a", "c"].map(|item| vec![Value::String(item.into())])
        );
        // Act
        let arrays = sql(
            cassie,
            session,
            "SELECT item FROM array_values ORDER BY arr",
        );
        // Assert
        assert_eq!(
            arrays.rows(),
            ["c", "d", "a", "b"].map(|item| vec![Value::String(item.into())])
        );
    });
}

#[test]
fn should_compare_qualified_array_parameters_without_retyping_json_columns() {
    // Arrange
    with_fixture("typed_array_qualified_predicate", |cassie, session| {
        seed_numeric_arrays(cassie, session);
        let parameter = Value::Json(serde_json::json!([2]));
        let json = cassie.execute_sql(session, "SELECT json_values.item FROM array_values JOIN json_values ON array_values.item=json_values.item WHERE json_values.arr > $1 ORDER BY json_values.item", vec![parameter.clone()]).expect("JSON comparison control");
        assert_eq!(json.rows(), vec![vec![Value::String("c".into())]]);
        // Act
        let arrays = cassie.execute_sql(session, "SELECT array_values.item FROM array_values JOIN json_values ON array_values.item=json_values.item WHERE array_values.arr > $1 ORDER BY array_values.item", vec![parameter]).expect("qualified ARRAY comparison");
        // Assert
        assert_eq!(arrays.rows(), vec![vec![Value::String("b".into())]]);
        for query in [
            "SELECT item FROM array_values WHERE COALESCE(arr, $1) > $2 ORDER BY item",
            "SELECT item FROM array_values WHERE arr > COALESCE($1, $2) ORDER BY item",
        ] {
            let result = cassie
                .execute_sql(
                    session,
                    query,
                    vec![
                        Value::Json(serde_json::json!([2])),
                        Value::Json(serde_json::json!([2])),
                    ],
                )
                .expect("contextual ARRAY parameter");
            assert_eq!(
                result.rows(),
                vec![vec![Value::String("b".into())]],
                "{query}"
            );
        }
        let lateral = cassie.execute_sql(session, "SELECT array_values.item FROM array_values JOIN LATERAL (SELECT item FROM json_values WHERE arr > $1 AND json_values.item=array_values.item) AS inner_values ON TRUE", vec![Value::Json(serde_json::json!([2]))]).expect("inner JSON shadows outer ARRAY type");
        assert_eq!(lateral.rows(), vec![vec![Value::String("c".into())]]);
    });
}
#[test]
fn should_keep_array_order_through_relational_execution() {
    // Arrange
    with_fixture("typed_array_source_paths", |cassie, session| {
        seed_numeric_arrays(cassie, session);
        sql(
            cassie,
            session,
            "CREATE VIEW array_view AS SELECT item, arr FROM array_values",
        );
        // Act
        // Assert
        for query in [
                "SELECT item FROM array_values ORDER BY arr LIMIT 3",
                "SELECT item FROM array_view ORDER BY arr LIMIT 3",
                "SELECT item FROM (SELECT item, arr FROM array_values) AS derived ORDER BY arr LIMIT 3",
                "WITH arrays AS (SELECT item, arr FROM array_values) SELECT item FROM arrays ORDER BY arr LIMIT 3",
                "SELECT item FROM array_values ORDER BY COALESCE(arr, NULL) LIMIT 3",
                "SELECT item FROM array_values ORDER BY CASE WHEN item='a' THEN arr ELSE arr END LIMIT 3",
            ] {
                assert_eq!(sql(cassie, session, query).rows(), ["c", "d", "a"].map(|item| vec![Value::String(item.into())]), "{query}");
            }
        assert_eq!(
            sql(
                cassie,
                session,
                "SELECT item FROM array_values ORDER BY arr DESC LIMIT 2"
            )
            .rows(),
            ["b", "a"].map(|item| vec![Value::String(item.into())])
        );
        assert_eq!(sql(cassie, session, "SELECT item, ROW_NUMBER() OVER (ORDER BY arr) AS rank FROM array_values ORDER BY arr").rows(), ["c", "d", "a", "b"].into_iter().zip(1..=4).map(|(item, rank)| vec![Value::String(item.into()), Value::Int64(rank)]).collect::<Vec<_>>());
        assert_eq!(sql(cassie, session, "SELECT item, FIRST_VALUE(arr) OVER (PARTITION BY item ORDER BY item) AS first_arr FROM array_values ORDER BY first_arr LIMIT 1").rows(), vec![vec![Value::String("c".into()), Value::Json(serde_json::json!([]))]]);
        assert_eq!(sql(cassie, session, "SELECT left_arrays.item FROM (SELECT item, arr FROM array_values) AS left_arrays JOIN (SELECT item, arr FROM array_values) AS right_arrays ON left_arrays.arr > right_arrays.arr WHERE right_arrays.item='a'").rows(), vec![vec![Value::String("b".into())]]);
        let bounded = cassie
            .execute_sql(
                session,
                "SELECT item FROM array_values WHERE arr BETWEEN $1 AND $2 ORDER BY arr",
                vec![
                    Value::Json(serde_json::json!([2])),
                    Value::Json(serde_json::json!([10])),
                ],
            )
            .expect("ARRAY BETWEEN");
        assert_eq!(
            bounded.rows(),
            ["a", "b"].map(|item| vec![Value::String(item.into())])
        );
    });
}

#[test]
fn should_order_array_boundaries_in_the_declared_element_domain() {
    // Arrange
    with_fixture("typed_array_edge_values", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE edge_arrays (item TEXT, arr BIGINT[])",
        );
        for (item, value) in [
            ("missing", Value::Null),
            ("empty", Value::Json(serde_json::json!([]))),
            ("short", Value::Json(serde_json::json!([1]))),
            ("long", Value::Json(serde_json::json!([1, 2]))),
            ("element_null", Value::Json(serde_json::json!([1, null]))),
            (
                "exact_low",
                Value::Json(serde_json::json!([9_007_199_254_740_992_i64])),
            ),
            (
                "exact_high",
                Value::Json(serde_json::json!([9_007_199_254_740_993_i64])),
            ),
        ] {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO edge_arrays (item, arr) VALUES ($1, $2)",
                    vec![Value::String(item.into()), value],
                )
                .expect("array edge fixture");
        }
        // Act
        let ordered = sql(
            cassie,
            session,
            "SELECT item FROM edge_arrays ORDER BY arr NULLS LAST",
        );
        let after = cassie
            .execute_sql(
                session,
                "SELECT item FROM edge_arrays WHERE arr > $1 ORDER BY arr",
                vec![Value::Json(serde_json::json!([9_007_199_254_740_992_i64]))],
            )
            .expect("exact ARRAY comparison");
        // Assert
        assert_eq!(
            ordered.rows(),
            [
                "empty",
                "short",
                "long",
                "element_null",
                "exact_low",
                "exact_high",
                "missing"
            ]
            .map(|item| vec![Value::String(item.into())])
        );
        assert_eq!(after.rows(), vec![vec![Value::String("exact_high".into())]]);
        assert_eq!(
            sql(
                cassie,
                session,
                "SELECT COUNT(*) FROM edge_arrays WHERE arr > NULL"
            )
            .rows(),
            vec![vec![Value::Int64(0)]]
        );
    });
}
#[test]
fn should_retain_array_types_through_optimized_execution() {
    // Arrange
    with_fixture("typed_array_aggregate_index", |cassie, session| {
        seed_numeric_arrays(cassie, session);
        // Act
        let grouped = sql(
            cassie,
            session,
            "SELECT item, MIN(arr) AS least_arr FROM array_values GROUP BY item ORDER BY least_arr",
        );
        // Assert
        assert_eq!(
            grouped
                .rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["c", "d", "a", "b"].map(|item| Value::String(item.into()))
        );
        sql(
            cassie,
            session,
            "CREATE INDEX array_order_idx ON array_values (arr)",
        );
        let indexed = cassie
            .execute_sql(
                session,
                "SELECT item FROM array_values WHERE arr > $1 ORDER BY arr LIMIT 2",
                vec![Value::Json(serde_json::json!([2]))],
            )
            .expect("ARRAY index range fallback");
        assert_eq!(indexed.rows(), vec![vec![Value::String("b".into())]]);
        sql(
            cassie,
            session,
            "DROP INDEX array_order_idx ON array_values",
        );
        sql(
            cassie,
            session,
            "CREATE INDEX array_expression_idx ON array_values ((COALESCE(arr, NULL)))",
        );
        let expression_indexed = cassie
            .execute_sql(
                session,
                "SELECT item FROM array_values WHERE COALESCE(arr, NULL) > $1 ORDER BY item",
                vec![Value::Json(serde_json::json!([2]))],
            )
            .expect("ARRAY expression index range fallback");
        assert_eq!(
            expression_indexed.rows(),
            vec![vec![Value::String("b".into())]]
        );
        let union = sql(
            cassie,
            session,
            "SELECT arr FROM array_values UNION SELECT arr FROM array_values ORDER BY arr",
        );
        assert_eq!(
            union.rows(),
            [
                serde_json::json!([]),
                serde_json::json!([1, 9]),
                serde_json::json!([2]),
                serde_json::json!([10])
            ]
            .map(|array| vec![Value::Json(array)])
        );
    });
}
#[test]
fn should_order_array_elements_in_their_declared_domains() {
    // Arrange
    with_fixture("typed_array_element_domains", |cassie, session| {
        for (table, data_type, first, last) in [
            (
                "boolean_arrays",
                "BOOLEAN[]",
                serde_json::json!([false]),
                serde_json::json!([true]),
            ),
            (
                "text_arrays",
                "TEXT[]",
                serde_json::json!(["a"]),
                serde_json::json!(["b"]),
            ),
            (
                "json_arrays",
                "JSON[]",
                serde_json::json!([10]),
                serde_json::json!([2]),
            ),
            (
                "vector_arrays",
                "VECTOR(2)[]",
                serde_json::json!([[-2.0, 0.0]]),
                serde_json::json!([[-1.0, 0.0]]),
            ),
        ] {
            sql(
                cassie,
                session,
                &format!("CREATE TABLE {table} (item TEXT, arr {data_type})"),
            );
            for (item, array) in [("last", last.clone()), ("first", first.clone())] {
                cassie
                    .execute_sql(
                        session,
                        &format!("INSERT INTO {table} (item, arr) VALUES ($1, $2)"),
                        vec![Value::String(item.into()), Value::Json(array)],
                    )
                    .expect("typed element fixture");
            }
            // Act
            let ordered = sql(
                cassie,
                session,
                &format!("SELECT item FROM {table} ORDER BY arr LIMIT 1"),
            );
            let extrema = sql(
                cassie,
                session,
                &format!("SELECT MIN(arr), MAX(arr) FROM {table}"),
            );
            // Assert
            assert_eq!(
                ordered.rows(),
                vec![vec![Value::String("first".into())]],
                "{data_type}"
            );
            assert_eq!(
                extrema.rows(),
                vec![vec![Value::Json(first), Value::Json(last)]],
                "{data_type}"
            );
        }
    });
}
#[test]
fn should_use_elementwise_array_peers_in_window_ranks() {
    // Arrange
    with_fixture("typed_array_window_peers", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE float_arrays (item TEXT, arr FLOAT[])",
        );
        for (item, array) in [
            ("a", serde_json::json!([-0.0])),
            ("b", serde_json::json!([0.0])),
            ("c", serde_json::json!([1.0])),
        ] {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO float_arrays (item, arr) VALUES ($1, $2)",
                    vec![Value::String(item.into()), Value::Json(array)],
                )
                .expect("numeric ARRAY peer fixture");
        }
        // Act
        let result = sql(cassie, session, "SELECT item, RANK() OVER (ORDER BY arr) AS ranking, DENSE_RANK() OVER (ORDER BY arr) AS dense FROM float_arrays ORDER BY item");
        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("a".into()), Value::Int64(1), Value::Int64(1)],
                vec![Value::String("b".into()), Value::Int64(1), Value::Int64(1)],
                vec![Value::String("c".into()), Value::Int64(3), Value::Int64(2)]
            ]
        );
        sql(
            cassie,
            session,
            "CREATE TABLE char_arrays (item TEXT, arr CHAR(8)[])",
        );
        for (item, array) in [
            ("a", serde_json::json!(["a"])),
            ("b", serde_json::json!(["a  "])),
            ("c", serde_json::json!(["b"])),
        ] {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO char_arrays (item, arr) VALUES ($1, $2)",
                    vec![Value::String(item.into()), Value::Json(array)],
                )
                .expect("CHAR ARRAY peer fixture");
        }
        let char_ranks = sql(cassie, session, "SELECT item, RANK() OVER (ORDER BY arr) AS ranking, DENSE_RANK() OVER (ORDER BY arr) AS dense FROM char_arrays ORDER BY item");
        assert_eq!(char_ranks.rows(), result.rows());
    });
}
#[test]
fn should_preserve_array_comparisons_in_mutation_expressions() {
    // Arrange
    with_fixture("typed_array_dml_contexts", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE array_dml (item TEXT PRIMARY KEY, arr INT[])",
        );
        let bound = Value::Json(serde_json::json!([2]));
        // Act
        // Assert
        for (item, array, greater) in [
            ("a", serde_json::json!([10]), true),
            ("b", serde_json::json!([]), false),
        ] {
            let inserted = cassie
                .execute_sql(
                    session,
                    "INSERT INTO array_dml VALUES ($1,$2) RETURNING item, arr > $3 AS greater",
                    vec![
                        Value::String(item.into()),
                        Value::Json(array),
                        bound.clone(),
                    ],
                )
                .expect("ARRAY INSERT RETURNING");
            assert_eq!(
                inserted.rows(),
                vec![vec![Value::String(item.into()), Value::Bool(greater)]]
            );
        }
        let conflict = cassie.execute_sql(session, "INSERT INTO array_dml VALUES ($1,$2) ON CONFLICT (item) DO UPDATE SET item=excluded.item WHERE array_dml.arr > $3 RETURNING item", vec![Value::String("a".into()), Value::Json(serde_json::json!([1])), bound.clone()]).expect("typed ARRAY conflict filter");
        assert_eq!(conflict.rows(), vec![vec![Value::String("a".into())]]);
        for predicate in [
            "array_dml.arr > excluded.arr",
            "array_dml.arr > COALESCE(excluded.arr,NULL)",
        ] {
            let query = format!("INSERT INTO array_dml VALUES ($1,$2) ON CONFLICT (item) DO UPDATE SET item=excluded.item WHERE {predicate} RETURNING item");
            let conflict = cassie
                .execute_sql(
                    session,
                    &query,
                    vec![
                        Value::String("a".into()),
                        Value::Json(serde_json::json!([1])),
                    ],
                )
                .expect("EXCLUDED ARRAY comparison");
            assert_eq!(
                conflict.rows(),
                vec![vec![Value::String("a".into())]],
                "{predicate}"
            );
        }
        for query in [
            "UPDATE array_dml SET item=item RETURNING item, arr > $1 AS greater",
            "DELETE FROM array_dml RETURNING item, arr > $1 AS greater",
        ] {
            let mut rows = cassie
                .execute_sql(session, query, vec![bound.clone()])
                .expect("ARRAY DML RETURNING")
                .rows()
                .to_vec();
            rows.sort_by_key(|row| row[0].as_str().expect("item text").to_owned());
            assert_eq!(
                rows,
                vec![
                    vec![Value::String("a".into()), Value::Bool(true)],
                    vec![Value::String("b".into()), Value::Bool(false)]
                ],
                "{query}"
            );
        }
    });
}
#[test]
fn should_retain_array_types_in_declared_id_outer_joins() {
    // Arrange
    with_fixture("typed_array_outer_ids", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE declared_arrays (id TEXT, arr INT[])",
        );
        sql(
            cassie,
            session,
            "CREATE TABLE empty_arrays (id TEXT, arr INT[])",
        );
        for (id, array) in [
            ("a", serde_json::json!([2])),
            ("b", serde_json::json!([10])),
            ("c", serde_json::json!([])),
            ("d", serde_json::json!([1, 9])),
        ] {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO declared_arrays VALUES ($1,$2)",
                    vec![Value::String(id.into()), Value::Json(array)],
                )
                .expect("declared id ARRAY fixture");
        }
        for kind in ["LEFT", "RIGHT", "FULL"] {
            let source = if kind == "RIGHT" {
                "empty_arrays RIGHT JOIN declared_arrays ON empty_arrays.id=declared_arrays.id"
                    .to_string()
            } else {
                format!("declared_arrays {kind} JOIN empty_arrays ON declared_arrays.id=empty_arrays.id")
            };
            for expr in [
                "declared_arrays.arr",
                "COALESCE(empty_arrays.arr,declared_arrays.arr)",
            ] {
                // Act
                let ordered = sql(
                    cassie,
                    session,
                    &format!("SELECT declared_arrays.id FROM {source} ORDER BY {expr}"),
                );
                let filtered = cassie.execute_sql(session, &format!("SELECT declared_arrays.id FROM {source} WHERE {expr} > $1 ORDER BY declared_arrays.id"), vec![Value::Json(serde_json::json!([2]))]).expect("outer ARRAY predicate");
                // Assert
                assert_eq!(
                    ordered.rows(),
                    ["c", "d", "a", "b"].map(|id| vec![Value::String(id.into())])
                );
                assert_eq!(filtered.rows(), vec![vec![Value::String("b".into())]]);
            }
        }
    });
}

#[test]
fn should_reject_array_returning_budget_exhaustion_before_publishing_writes() {
    // Arrange
    with_fixture("typed_array_returning_budget", |cassie, session| {
        sql(cassie, session, "CREATE TABLE array_budget (arr INT[])");
        let plan = cassie
            .compile_sql_physical_plan_for_diagnostics(
                "INSERT INTO array_budget (arr) VALUES ($1) RETURNING arr",
            )
            .expect("ARRAY RETURNING plan");
        let limits = cassie::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 64,
            ..cassie::config::CassieRuntimeLimits::default()
        };
        let controls = cassie::runtime::QueryExecutionControls::from_limits(
            &limits,
            std::time::Instant::now(),
        );
        // Act
        let result = cassie::executor::run_with_controls(
            cassie,
            &plan,
            vec![Value::Json(serde_json::json!((0..100).collect::<Vec<_>>()))],
            &controls,
        );
        // Assert
        let error =
            result.expect_err("retained ARRAY RETURNING rows must respect the query budget");
        assert!(error.to_string().contains("query memory budget exceeded"));
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(
            sql(cassie, session, "SELECT COUNT(*) FROM array_budget").rows(),
            vec![vec![Value::Int64(0)]]
        );
    });
}
