use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

use support::*;

#[test]
fn should_scan_scalar_index_with_signed_float_bounds() {
    // Arrange
    let path = data_dir("scalar_lexkey_numeric_bounds");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE scalar_lexkey_numeric_bounds (score INT, rating FLOAT)",
                    vec![],
                )
                .unwrap();
            for (id, score, rating) in [
                ("row-1", -10, -2.5),
                ("row-2", -2, -1.25),
                ("row-3", 0, 0.5),
                ("row-4", 7, 3.75),
            ] {
                cassie
                    .midge
                    .put_document(
                        "scalar_lexkey_numeric_bounds",
                        Some(id.to_string()),
                        serde_json::json!({"score": score, "rating": rating}),
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX scalar_lexkey_score_idx ON scalar_lexkey_numeric_bounds USING btree (score)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX scalar_lexkey_rating_idx ON scalar_lexkey_numeric_bounds USING btree (rating)",
                    vec![],
                )
                .unwrap();

            // Act
            let scores = cassie
                .execute_sql(
                    &session,
                    "SELECT score FROM scalar_lexkey_numeric_bounds WHERE score >= -2 AND score < 7 ORDER BY score",
                    vec![],
                )
                .unwrap();
            let ratings = cassie
                .execute_sql(
                    &session,
                    "SELECT rating FROM scalar_lexkey_numeric_bounds WHERE rating > -2.5 AND rating <= 0.5 ORDER BY rating",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                scores.rows(),
                vec![vec![Value::Int64(-2)], vec![Value::Int64(0)]]
            );
            assert_eq!(
                ratings.rows(),
                vec![vec![Value::Float64(-1.25)], vec![Value::Float64(0.5)]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_match_full_scan_for_signed_zero_float_scalar_index_constraints() {
    // Arrange
    let path = data_dir("scalar_lexkey_negative_zero_equality");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for (table, indexed) in [
            ("negative_zero_baseline", false),
            ("negative_zero_indexed", true),
        ] {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {table} (tag TEXT, delta FLOAT)"),
                    vec![],
                )
                .expect("create float table");
            for (tag, delta) in [
                ("negative", -1.5),
                ("negative-zero", -0.0),
                ("positive-zero", 0.0),
                ("positive", 1.5),
            ] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("INSERT INTO {table} (tag, delta) VALUES ($1, $2)"),
                        vec![Value::String(tag.to_owned()), Value::Float64(delta)],
                    )
                    .expect("insert signed zero");
            }
            if indexed {
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "CREATE INDEX negative_zero_delta_idx ON {table} USING btree (delta)"
                        ),
                        vec![],
                    )
                    .expect("create float scalar index");
            }
        }

        // Act
        for (predicate, expected_tags) in [
            ("=", vec!["negative-zero", "positive-zero"]),
            (">=", vec!["negative-zero", "positive-zero", "positive"]),
            ("<", vec!["negative"]),
        ] {
            let baseline = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT tag FROM negative_zero_baseline WHERE delta {predicate} $1"),
                vec![Value::Float64(0.0)],
            );
            let indexed = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT tag FROM negative_zero_indexed WHERE delta {predicate} $1"),
                vec![Value::Float64(0.0)],
            );
            let mut expected = expected_tags
                .into_iter()
                .map(|tag| vec![Value::String(tag.to_owned())])
                .collect::<Vec<_>>();
            expected.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));

            // Assert
            assert_eq!(baseline, expected);
            assert_eq!(indexed, baseline, "delta {predicate} 0.0");
        }

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_match_full_scan_for_signed_zero_float_expression_index() {
    // Arrange
    let path = data_dir("scalar_lexkey_signed_zero_expression_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            let session = cassie.create_session("tester", None);
            for (table, indexed) in [
                ("signed_zero_expression_baseline", false),
                ("signed_zero_expression_indexed", true),
            ] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("CREATE TABLE {table} (tag TEXT, delta FLOAT)"),
                        vec![],
                    )
                    .expect("create float table");
                for (tag, delta) in [("negative", -0.0), ("positive", 0.0)] {
                    cassie
                        .execute_sql(
                            &session,
                            &format!("INSERT INTO {table} (tag, delta) VALUES ($1, $2)"),
                            vec![Value::String(tag.to_owned()), Value::Float64(delta)],
                        )
                        .expect("insert signed zero");
                }
                if indexed {
                    cassie
                        .execute_sql(
                            &session,
                            &format!(
                                "CREATE INDEX signed_zero_expression_idx ON {table} USING btree ((delta * 1.0))"
                            ),
                            vec![],
                        )
                        .expect("create float expression index");
                }
            }

            // Act
            let baseline = sorted_rows(
                &cassie,
                &session,
                "SELECT tag FROM signed_zero_expression_baseline WHERE delta * 1.0 = $1",
                vec![Value::Float64(0.0)],
            );
            let indexed = sorted_rows(
                &cassie,
                &session,
                "SELECT tag FROM signed_zero_expression_indexed WHERE delta * 1.0 = $1",
                vec![Value::Float64(0.0)],
            );

            // Assert
            assert_eq!(
                baseline,
                vec![
                    vec![Value::String("negative".to_owned())],
                    vec![Value::String("positive".to_owned())]
                ]
            );
            assert_eq!(indexed, baseline);

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_match_full_scan_for_numeric_expression_index_bounds() {
    // Arrange
    let path = data_dir("scalar_lexkey_numeric_expression_bounds");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for (table, indexed) in [
            ("numeric_expression_baseline", false),
            ("numeric_expression_indexed", true),
        ] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {table} (x FLOAT)"), vec![])
                .expect("create float table");
            for value in [-1.0, 2.5, 5.0, 7.25] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("INSERT INTO {table} (x) VALUES ($1)"),
                        vec![Value::Float64(value)],
                    )
                    .expect("insert float row");
            }
            if indexed {
                for (name, expression) in [
                    ("numeric_expression_times_two_idx", "x * 2"),
                    ("numeric_expression_abs_idx", "abs(x)"),
                ] {
                    cassie
                        .execute_sql(
                            &session,
                            &format!("CREATE INDEX {name} ON {table} USING btree (({expression}))"),
                            vec![],
                        )
                        .expect("create numeric expression index");
                }
            }
        }

        // Act
        let mut mismatches = Vec::new();
        for predicate in [
            "x * 2 = 5",
            "x * 2 > 4 AND x * 2 < 11",
            "abs(x) > 1",
            "abs(x) < 3",
        ] {
            let baseline = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT x FROM numeric_expression_baseline WHERE {predicate}"),
                vec![],
            );
            let indexed = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT x FROM numeric_expression_indexed WHERE {predicate}"),
                vec![],
            );
            if baseline != indexed {
                mismatches.push(format!(
                    "{predicate}: baseline={baseline:?} indexed={indexed:?}"
                ));
            }
        }

        // Assert
        assert!(
            mismatches.is_empty(),
            "numeric expression indexes must agree with full scans: {mismatches:?}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

fn sorted_rows(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    sql: &str,
    params: Vec<Value>,
) -> Vec<Vec<Value>> {
    let mut rows = cassie
        .execute_sql(session, sql, params)
        .expect(sql)
        .rows()
        .to_vec();
    rows.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
    rows
}

fn seed_float_tables_with_index_first(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for (table, indexed) in [
        ("float_index_first_baseline", false),
        ("float_index_first_indexed", true),
    ] {
        cassie
            .execute_sql(session, &format!("CREATE TABLE {table} (x FLOAT)"), vec![])
            .expect("create float table");
        if indexed {
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE INDEX float_index_first_x_idx ON {table} USING btree (x)"),
                    vec![],
                )
                .expect("create float scalar index before writes");
        }
        for sql in [
            format!("INSERT INTO {table} (x) VALUES (5)"),
            format!("INSERT INTO {table} (x) VALUES (10)"),
            format!("INSERT INTO {table} (x) VALUES (2.5)"),
            format!("UPDATE {table} SET x = 12 WHERE x = 10"),
        ] {
            cassie.execute_sql(session, &sql, vec![]).expect(&sql);
        }
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {table} (x) VALUES ($1)"),
                vec![Value::Int64(7)],
            )
            .expect("insert integer parameter");
    }
}

#[test]
fn should_match_unindexed_float_results_when_scalar_index_precedes_writes() {
    // Arrange
    let path = data_dir("scalar_lexkey_float_index_first");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        seed_float_tables_with_index_first(&cassie, &session);
        let literal_predicates = [
            "x > 4.5", "x >= 5", "x < 0.5", "x <= 7", "x = 5", "x = 7", "x = 10", "x > 11",
        ];
        let parameter_predicates = [
            ("x > $1", 5),
            ("x >= $1", 5),
            ("x < $1", 7),
            ("x <= $1", 7),
            ("x = $1", 12),
        ];

        // Act
        let mut mismatches = Vec::new();
        for predicate in literal_predicates {
            let baseline = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT x FROM float_index_first_baseline WHERE {predicate}"),
                vec![],
            );
            let indexed = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT x FROM float_index_first_indexed WHERE {predicate}"),
                vec![],
            );
            if indexed != baseline {
                mismatches.push(format!(
                    "{predicate}: baseline={baseline:?} indexed={indexed:?}"
                ));
            }
        }
        for (predicate, parameter) in parameter_predicates {
            let baseline = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT x FROM float_index_first_baseline WHERE {predicate}"),
                vec![Value::Int64(parameter)],
            );
            let indexed = sorted_rows(
                &cassie,
                &session,
                &format!("SELECT x FROM float_index_first_indexed WHERE {predicate}"),
                vec![Value::Int64(parameter)],
            );
            if indexed != baseline {
                mismatches.push(format!(
                    "{predicate} [{parameter}]: baseline={baseline:?} indexed={indexed:?}"
                ));
            }
        }
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT x FROM float_index_first_indexed WHERE x > 4.5",
                vec![],
            )
            .expect("explain float range");

        // Assert
        assert!(
            mismatches.is_empty(),
            "indexed FLOAT results diverge from unindexed results: {mismatches:#?}"
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=float_index_first_x_idx"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_match_integer_shaped_literals_in_float_scalar_indexes() {
    // Arrange
    let path = data_dir("scalar_lexkey_whole_float");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for table in ["whole_float_baseline", "whole_float_indexed"] {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {table} (row_number INT, rating FLOAT)"),
                    vec![],
                )
                .expect("create float table");
            cassie
                .execute_sql(
                    &session,
                    &format!("INSERT INTO {table} (row_number, rating) VALUES (1, 100.0)"),
                    vec![],
                )
                .expect("insert whole-number float");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX whole_float_rating_idx ON whole_float_indexed USING btree (rating)",
                vec![],
            )
            .expect("create float scalar index");

        // Act
        let baseline_integer = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_baseline WHERE rating = 100",
                vec![],
            )
            .expect("query unindexed float with integer literal");
        let indexed_integer = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_indexed WHERE rating = 100",
                vec![],
            )
            .expect("query indexed float with integer literal");
        let baseline_float = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_baseline WHERE rating = 100.0",
                vec![],
            )
            .expect("query unindexed float with float literal");
        let indexed_float = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_indexed WHERE rating = 100.0",
                vec![],
            )
            .expect("query indexed float with float literal");
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT row_number FROM whole_float_indexed WHERE rating = 100",
                vec![],
            )
            .expect("explain float scalar index lookup");

        // Assert
        let expected = vec![vec![Value::Int64(1)]];
        assert_eq!(baseline_integer.rows(), expected);
        assert_eq!(indexed_integer.rows(), baseline_integer.rows());
        assert_eq!(baseline_float.rows(), expected);
        assert_eq!(indexed_float.rows(), baseline_float.rows());
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=whole_float_rating_idx"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_match_full_scan_for_fractional_bounds_on_integer_scalar_indexes() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for table in ["fractional_int_baseline", "fractional_int_indexed"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {table} (n INT)"), vec![])
                .expect("create integer table");
            cassie
                .execute_sql(
                    &session,
                    &format!("INSERT INTO {table} (n) VALUES (1), (5), (6), (100)"),
                    vec![],
                )
                .expect("insert integer rows");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX fractional_int_n_idx ON fractional_int_indexed USING btree (n)",
                vec![],
            )
            .expect("create integer scalar index");

        let predicates = [
            "n > 5.5",
            "n >= 5.5",
            "n < 5.5",
            "n <= 5.5",
            "n > 5.0",
            "n >= 5.0",
            "n BETWEEN 1.5 AND 5.5",
            "n = 5.5",
            "n = 5.0",
        ];

        // Act
        let mut divergent = Vec::new();
        for predicate in predicates {
            let baseline = cassie
                .execute_sql(
                    &session,
                    &format!("SELECT n FROM fractional_int_baseline WHERE {predicate} ORDER BY n"),
                    vec![],
                )
                .expect("query unindexed integer column");
            let indexed = cassie
                .execute_sql(
                    &session,
                    &format!("SELECT n FROM fractional_int_indexed WHERE {predicate} ORDER BY n"),
                    vec![],
                )
                .expect("query indexed integer column");
            if baseline.rows() != indexed.rows() {
                divergent.push(format!(
                    "{predicate}: unindexed {:?} but indexed {:?}",
                    baseline.rows(),
                    indexed.rows()
                ));
            }
        }

        // Assert
        assert!(
            divergent.is_empty(),
            "index-backed reads must agree with full scans: {divergent:?}"
        );
    });
}

#[test]
fn should_match_full_scan_for_large_integral_float_bounds_on_bigint_indexes() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        let boundary = 1_i64 << 62;
        let float_boundary = 4_611_686_018_427_387_904.0;
        for table in ["large_float_baseline", "large_float_indexed"] {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {table} (n BIGINT)"),
                    vec![],
                )
                .expect("create BIGINT table");
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO {table} (n) VALUES ({}), ({boundary}), ({})",
                        boundary - 1,
                        boundary + 1
                    ),
                    vec![],
                )
                .expect("insert BIGINT rows");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX large_float_n_idx ON large_float_indexed USING btree (n)",
                vec![],
            )
            .expect("create BIGINT scalar index");

        // Act
        let mut divergent = Vec::new();
        for predicate in ["n > $1", "n < $1"] {
            let parameter = vec![Value::Float64(float_boundary)];
            let baseline = cassie
                .execute_sql(
                    &session,
                    &format!("SELECT n FROM large_float_baseline WHERE {predicate} ORDER BY n"),
                    parameter.clone(),
                )
                .expect("query unindexed BIGINT column");
            let indexed = cassie
                .execute_sql(
                    &session,
                    &format!("SELECT n FROM large_float_indexed WHERE {predicate} ORDER BY n"),
                    parameter,
                )
                .expect("query indexed BIGINT column");
            if baseline.rows() != indexed.rows() {
                divergent.push(format!(
                    "{predicate}: unindexed {:?} but indexed {:?}",
                    baseline.rows(),
                    indexed.rows()
                ));
            }
        }

        // Assert
        assert!(
            divergent.is_empty(),
            "index-backed reads must agree with full scans: {divergent:?}"
        );
    });
}

#[test]
fn should_scan_composite_scalar_index_with_embedded_nul_text() {
    // Arrange
    let path = data_dir("scalar_lexkey_nul_text");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE scalar_lexkey_nul_text (tenant TEXT, label TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "scalar_lexkey_nul_text",
                    Some("row-1".to_string()),
                    serde_json::json!({"tenant": "acme", "label": "aa"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "scalar_lexkey_nul_text",
                    Some("row-2".to_string()),
                    serde_json::json!({"tenant": "acme", "label": "a\u{0}a"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX scalar_lexkey_tenant_label_idx ON scalar_lexkey_nul_text USING btree (tenant, label)",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM scalar_lexkey_nul_text WHERE tenant = 'acme' AND label >= 'a' ORDER BY label",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT label FROM scalar_lexkey_nul_text WHERE tenant = 'acme' AND label >= 'a' ORDER BY label",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                result.rows(),
                vec![
                    vec![Value::String("a\u{0}a".to_string())],
                    vec![Value::String("aa".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows()[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=scalar_lexkey_tenant_label_idx"));

            let _ = std::fs::remove_dir_all(path);
        });
}

fn seed_same_second_timestamp_tables(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for (table, indexed) in [
        ("same_second_ts_baseline", false),
        ("same_second_ts_indexed", true),
    ] {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {table} (seq INT, ts TIMESTAMP)"),
                vec![],
            )
            .expect("create timestamp table");
        if indexed {
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE INDEX same_second_ts_idx ON {table} USING btree (ts)"),
                    vec![],
                )
                .expect("create timestamp scalar index");
        }
        // Chronological order is 5, 1, 3, 2, 4, 6: whole-second and
        // fractional instants share the 12:00:00 second.
        for (seq, ts) in [
            (1, "2024-01-01T12:00:00Z"),
            (2, "2024-01-01T12:00:00.500000Z"),
            (3, "2024-01-01T12:00:00.25Z"),
            (4, "2024-01-01T13:00:00.75+01:00"),
            (5, "2024-01-01T11:59:59.999999Z"),
            (6, "2024-01-01 12:00:01"),
        ] {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO {table} (seq, ts) VALUES ({seq}, '{ts}')"),
                    vec![],
                )
                .expect("insert timestamp row");
        }
    }
}

fn seq_column(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> Vec<i64> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect(sql)
        .rows()
        .iter()
        .map(|row| match row.first() {
            Some(Value::Int64(seq)) => *seq,
            other => panic!("expected integer seq, got {other:?}"),
        })
        .collect()
}

#[test]
fn should_preserve_same_second_timestamp_order_across_index_paths() {
    // Arrange
    let path = data_dir("scalar_lexkey_same_second_timestamps");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            let session = cassie.create_session("tester", None);
            seed_same_second_timestamp_tables(&cassie, &session);
            let cases = [
                ("ORDER BY ts ASC", vec![5, 1, 3, 2, 4, 6]),
                ("ORDER BY ts DESC", vec![6, 4, 2, 3, 1, 5]),
                ("ORDER BY ts ASC LIMIT 2", vec![5, 1]),
                ("ORDER BY ts DESC LIMIT 2", vec![6, 4]),
                ("WHERE ts > '2024-01-01T12:00:00Z' ORDER BY seq", vec![2, 3, 4, 6]),
                ("WHERE ts >= '2024-01-01T12:00:00Z' ORDER BY seq", vec![1, 2, 3, 4, 6]),
                ("WHERE ts < '2024-01-01T12:00:00.5Z' ORDER BY seq", vec![1, 3, 5]),
                ("WHERE ts <= '2024-01-01T12:00:00Z' ORDER BY seq", vec![1, 5]),
                ("WHERE ts = '2024-01-01T12:00:00Z' ORDER BY seq", vec![1]),
                ("WHERE ts = '2024-01-01T12:00:00.25Z' ORDER BY seq", vec![3]),
                (
                    "WHERE ts > '2024-01-01T12:00:00Z' AND ts < '2024-01-01T12:00:01Z' ORDER BY seq",
                    vec![2, 3, 4],
                ),
            ];

            // Act
            let mut mismatches = Vec::new();
            for (clause, expected) in &cases {
                for table in ["same_second_ts_baseline", "same_second_ts_indexed"] {
                    let actual =
                        seq_column(&cassie, &session, &format!("SELECT seq FROM {table} {clause}"));
                    if &actual != expected {
                        mismatches.push(format!("{table} {clause}: expected {expected:?}, got {actual:?}"));
                    }
                }
            }
            let min_max = cassie
                .execute_sql(
                    &session,
                    "SELECT MIN(ts), MAX(ts) FROM same_second_ts_baseline WHERE ts >= '2024-01-01T12:00:00Z' AND ts < '2024-01-01T12:00:01Z'",
                    vec![],
                )
                .expect("min/max");
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT seq FROM same_second_ts_indexed WHERE ts > '2024-01-01T12:00:00Z'",
                    vec![],
                )
                .expect("explain timestamp range");

            // Assert
            assert!(
                mismatches.is_empty(),
                "timestamp results are not chronological: {mismatches:#?}"
            );
            assert_eq!(
                min_max.rows(),
                vec![vec![
                    Value::String("2024-01-01 12:00:00".to_string()),
                    Value::String("2024-01-01 12:00:00.75".to_string()),
                ]]
            );
            let Value::String(plan) = &explain.rows()[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=same_second_ts_idx"), "{plan}");

            let _ = std::fs::remove_dir_all(path);
        });
}
