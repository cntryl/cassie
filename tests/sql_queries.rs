// Consolidated integration suite: sql_queries.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/relational_evidence.rs"]
mod support_relational_evidence;
#[path = "support/sql.rs"]
mod support_sql;

mod relational_promotion_evidence {
    use super::support_relational_evidence::SeededRelationalFixture;

    #[test]
    fn should_preserve_seeded_relational_results_under_supported_rewrites() {
        // Arrange
        let forward = SeededRelationalFixture::from_seed(0x51_11CE, false).execute();
        let reverse = SeededRelationalFixture::from_seed(0x51_11CE, true).execute();

        // Act
        let join_rewrite = (&forward.keyed_join, &forward.filtered_cross_join);
        let cte_rewrite = (&forward.cte, &forward.direct);

        // Assert
        assert_eq!(join_rewrite.0, join_rewrite.1);
        assert_eq!(cte_rewrite.0, cte_rewrite.1);
        assert_eq!(forward.keyed_join, reverse.keyed_join);
        assert_eq!(forward.cte, reverse.cte);
        assert_eq!(forward.window, reverse.window);
        assert_eq!(forward.window, forward.expected_window);
        assert!(forward.has_null_window_value);
        assert!(forward.has_tied_window_rank);
        assert_eq!(forward.aggregate, forward.expected_aggregate);
        assert_eq!(forward.aggregate, reverse.aggregate);
    }
}

// Formerly tests/integration_sql_aggregates.rs.
mod integration_sql_aggregates {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

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
            selected.rows,
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
            selected.rows,
            vec![vec![
                Value::Int64(15),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_ignore_null_values_for_basic_aggregates_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("aggregate_nulls");
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
                "CREATE TABLE aggregate_null_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_null_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (NULL)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_null_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::Int64(2),
                Value::Int64(10),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
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
            selected.rows,
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
            selected.rows,
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
            selected.rows,
            vec![vec![Value::String("a".to_string()), Value::Int64(12)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_ctes.rs.
mod integration_sql_ctes {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_sql_with_non_recursive_cte() {
        // Arrange
        use_local_storage();
        let path = data_dir("cte_non_recursive");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "integration_cte";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "hello"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "world"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH docs_cte AS (SELECT title FROM integration_cte WHERE title = 'alpha') SELECT title FROM docs_cte",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], cassie::types::Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_sql_with_recursive_cte() {
        // Arrange
        use_local_storage();
        let path = data_dir("cte_recursive");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "integration_recursive_cte";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "n".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(collection, Some("d1".to_string()), serde_json::json!({"n": 1}))

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE counter(n) AS (SELECT n FROM integration_recursive_cte WHERE n = 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM counter WHERE n < 2) SELECT n FROM counter ORDER BY n",
            vec![],
            )

.unwrap();

        let rows = result
            .rows
            .into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                _ => panic!("expected integer value"),
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(rows, vec![1, 2]);
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_explain.rs.
mod integration_sql_explain {
    #![allow(unused_imports, dead_code)]

    use super::support_sql as support;
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use support::*;

    #[test]
    fn should_explain_select_query_plan() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_select_plan");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_explain_select_plan";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie
                .register_collection(
                    collection,
                    schema
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.data_type.clone()))
                        .collect(),
                );
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM sql_explain_select_plan WHERE title = 'alpha' ORDER BY title LIMIT 1",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.columns.len(), 1);
            assert_eq!(result.columns[0].name, "QUERY PLAN");
            assert_eq!(result.rows.len(), 1);
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("collection=sql_explain_select_plan"));
            assert!(plan.contains("operators=Scan>Filter>Sort>Project>Limit"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_predicate_pushdown_for_literal_equality_filter() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_predicate_pushdown");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_explain_predicate_pushdown";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM sql_explain_predicate_pushdown WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();

            // Assert
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("predicate_pushdown=true"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_read_path_metadata_in_explain() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_read_path_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_explain_read_path_metadata";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT id, title FROM sql_explain_read_path_metadata WHERE id = 'doc-1'",
                    vec![],
                )
                .unwrap();

            // Assert
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("access_path=point_lookup"));
            assert!(plan.contains("access_path_reason=point-lookup-id"));
            assert!(plan.contains("fallback_reason=none"));
            assert!(plan.contains("pagination_strategy=none"));
            assert!(plan.contains("top_k_mode=none"));
            assert!(plan.contains("early_stop=point_lookup"));
            assert!(plan.contains("projection_shape=materialized_projection"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_materialized_projection_freshness() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_materialized_projection");
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
                    "CREATE TABLE sql_explain_projection_source (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_explain_projection_source (title, score) VALUES ('alpha', 1)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE MATERIALIZED PROJECTION sql_explain_projection_ready AS SELECT title, score FROM sql_explain_projection_source",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM sql_explain_projection_ready ORDER BY title",
                    vec![],
                )
                .unwrap();

            // Assert
            let plan = explain_plan_text(&result);
            assert_explain_contains(plan, "projection_freshness", "fresh");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_projection_pruning_fields() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_projection_pruning");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_explain_projection_pruning";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "summary".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM sql_explain_projection_pruning WHERE body = 'alpha'",
                    vec![],
                )
                .unwrap();

            // Assert
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("projection_pruning=true"));
            assert!(plan.contains("scan_fields=title,body"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_limit_pushdown_scan_limit() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_limit_pushdown");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_explain_limit_pushdown";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM sql_explain_limit_pushdown LIMIT 20 OFFSET 5",
                    vec![],
                )
                .unwrap();

            // Assert
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("limit_pushdown=true"));
            assert!(plan.contains("scan_limit=25"));
            assert!(plan.contains("early_stop=scan_limit"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_top_k_plan_for_order_limit_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_top_k");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_explain_top_k (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM sql_explain_top_k ORDER BY score DESC LIMIT 5",
                    vec![],
                )
                .unwrap();

            // Assert
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("top_k=true"));
            assert!(plan.contains("top_k_limit=5"));
            assert!(plan.contains("top_k_mode=heap"));
            assert!(plan.contains("early_stop=none"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_storage_top_k_read_path_for_row_id_order() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_storage_top_k");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_explain_storage_top_k (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT id, title FROM sql_explain_storage_top_k ORDER BY id ASC LIMIT 5",
                    vec![],
                )
                .unwrap();

            // Assert
            let plan = explain_plan_text(&result);
            assert_explain_contains(plan, "access_path_reason", "row-key-top-k");
            assert_explain_contains(plan, "pagination_strategy", "limit");
            assert_explain_contains(plan, "top_k_mode", "storage");
            assert_explain_contains(plan, "early_stop", "storage_top_k");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_keyset_read_path_for_row_id_cursor() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_keyset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_explain_keyset (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT id, title FROM sql_explain_keyset WHERE id > 'doc-1' ORDER BY id ASC LIMIT 5",
                    vec![],
                )
                .unwrap();

            // Assert
            let plan = explain_plan_text(&result);
            assert_explain_contains(plan, "access_path_reason", "row-key-keyset");
            assert_explain_contains(plan, "pagination_strategy", "keyset");
            assert_explain_contains(plan, "top_k_mode", "none");
            assert_explain_contains(plan, "early_stop", "keyset");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_explain_analyze_select_query_plan() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_analyze_select");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_explain_analyze_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_explain_analyze_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN ANALYZE SELECT title FROM sql_explain_analyze_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();

            // Assert
            let Value::String(plan) = &result.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("analyze=true"));
            assert!(plan.contains("actual_rows=1"));
            assert!(plan.contains("diagnostics="));
            assert!(plan.contains("storage_reads_delta:"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/integration_sql_join_plans.rs.
mod integration_sql_join_plans {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    fn vectorized_join_config() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = 2;
        config
    }

    #[test]
    fn should_explain_hash_join_strategy_for_inner_equi_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_hash_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_hash_join_users (user_key TEXT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_hash_join_orders (order_user_key TEXT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_hash_join_users.name, sql_hash_join_orders.total FROM sql_hash_join_users JOIN sql_hash_join_orders ON sql_hash_join_users.user_key = sql_hash_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=hash"));
        assert!(plan.contains("projection_shape=runtime_join_degraded"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_semi_join_strategy_for_exists_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_semi_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_semi_join_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_semi_join_inner (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_semi_join_outer WHERE EXISTS (SELECT title FROM sql_semi_join_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=semi"));
        assert!(plan.contains("early_stop=exists"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_anti_join_strategy_for_not_exists_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_anti_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_anti_join_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_anti_join_inner (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_anti_join_outer WHERE NOT EXISTS (SELECT title FROM sql_anti_join_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=anti"));
        assert!(plan.contains("early_stop=exists"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_merge_join_strategy_when_ordering_matches_equi_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_merge_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_merge_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_merge_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_merge_join_users.name, sql_merge_join_orders.total FROM sql_merge_join_users JOIN sql_merge_join_orders ON sql_merge_join_users.user_key = sql_merge_join_orders.order_user_key ORDER BY sql_merge_join_users.user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=merge"));
        assert!(plan.contains("join_keys=sql_merge_join_users.user_key=sql_merge_join_orders.order_user_key"));
        assert!(plan.contains("join_sort_required=true"));
        assert!(plan.contains("join_fallback_reason=none"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_not_select_merge_join_for_non_equi_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_non_equi_join_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_non_equi_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_non_equi_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_non_equi_join_users.name, sql_non_equi_join_orders.total FROM sql_non_equi_join_users JOIN sql_non_equi_join_orders ON sql_non_equi_join_users.user_key > sql_non_equi_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=nested_loop"));
        assert!(plan.contains("join_fallback_reason=non_equi_predicate"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_vectorized_join_enabled_for_inner_equi_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_vectorized_join_enabled");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_vector_join_users.name, sql_vector_join_orders.total FROM sql_vector_join_users JOIN sql_vector_join_orders ON sql_vector_join_users.user_key = sql_vector_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("vectorized_join_candidate=true"));
        assert!(plan.contains("vectorized_join_enabled=true"));
        assert!(plan.contains("vectorized_join_batch_size=2"));
        assert!(plan.contains("vectorized_join_fallback_reason=none"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_vectorized_join_fallback_for_unsupported_join_type() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_vectorized_join_unsupported");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_full_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_full_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_vector_full_users.name, sql_vector_full_orders.total FROM sql_vector_full_users FULL OUTER JOIN sql_vector_full_orders ON sql_vector_full_users.user_key = sql_vector_full_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("vectorized_join_candidate=false"));
        assert!(plan.contains("vectorized_join_enabled=false"));
        assert!(plan.contains("vectorized_join_fallback_reason=unsupported_join_type"));

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_joins.rs.
mod integration_sql_joins {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig, OperatorSwitchingEnabled,
    };
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    fn vectorized_join_config() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = 2;
        config
    }

    fn operator_switch_join_config(enabled: bool, threshold: usize) -> CassieRuntimeConfig {
        let mut config = vectorized_join_config();
        config.limits.operator_switching_enabled = if enabled {
            OperatorSwitchingEnabled::enabled()
        } else {
            OperatorSwitchingEnabled::disabled()
        };
        config.limits.operator_switch_join_row_threshold = threshold;
        config
    }

    #[test]
    fn should_execute_inner_join_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_inner");
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
                "CREATE TABLE join_users (user_key INT, name TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO join_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO join_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT join_users.name, join_orders.total FROM join_users JOIN join_orders ON join_users.user_key = join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Int64(42)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_left_join_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_left");
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
                "CREATE TABLE left_users (user_key INT, name TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE left_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO left_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT left_users.name, left_orders.total FROM left_users LEFT JOIN left_orders ON left_users.user_key = left_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Null]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_not_join_null_keys_in_merge_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_merge_duplicate_null_keys");
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
                "CREATE TABLE merge_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE merge_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO merge_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO merge_users (user_key, name) VALUES (1, 'ada-alt')",
            "INSERT INTO merge_users (user_key, name) VALUES (NULL, 'unknown')",
            "INSERT INTO merge_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO merge_orders (order_user_key, total) VALUES (1, 99)",
            "INSERT INTO merge_orders (order_user_key, total) VALUES (NULL, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT merge_users.name, merge_orders.total FROM merge_users JOIN merge_orders ON merge_users.user_key = merge_orders.order_user_key ORDER BY merge_users.name, merge_orders.total",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("ada".to_string()), Value::Int64(99)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(42)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(99)]
            ]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["last_strategy"], "merge");
        assert_eq!(metrics["joins"]["merge_joins"], 1);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_not_join_null_keys_in_vectorized_inner_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_vectorized_inner_duplicate_null_keys");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO vector_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO vector_users (user_key, name) VALUES (1, 'ada-alt')",
            "INSERT INTO vector_users (user_key, name) VALUES (NULL, 'unknown')",
            "INSERT INTO vector_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO vector_orders (order_user_key, total) VALUES (1, 99)",
            "INSERT INTO vector_orders (order_user_key, total) VALUES (NULL, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT vector_users.name, vector_orders.total FROM vector_users JOIN vector_orders ON vector_users.user_key = vector_orders.order_user_key ORDER BY vector_users.name, vector_orders.total",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("ada".to_string()), Value::Int64(99)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(42)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(99)]
            ]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["last_strategy"], "vectorized");
        assert_eq!(metrics["joins"]["vectorized_joins"], 1);
        assert_eq!(metrics["joins"]["last_vectorized_batch_size"], 2);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_vectorized_left_join_unmatched_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_vectorized_left_unmatched");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_left_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_left_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO vector_left_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO vector_left_users (user_key, name) VALUES (2, 'grace')",
            "INSERT INTO vector_left_orders (order_user_key, total) VALUES (1, 42)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT vector_left_users.name, vector_left_orders.total FROM vector_left_users LEFT JOIN vector_left_orders ON vector_left_users.user_key = vector_left_orders.order_user_key ORDER BY vector_left_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Null]
            ]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["last_strategy"], "vectorized");
        assert_eq!(metrics["joins"]["vectorized_probe_rows_total"], 2);
        assert_eq!(metrics["joins"]["vectorized_build_rows_total"], 1);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_join_results_when_operator_switch_replays_inputs() {
        // Arrange
        use_local_storage();
        let fixed_path = data_dir("join_operator_switch_fixed");
        let switched_path = data_dir("join_operator_switch_replay");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let fixed =
            Cassie::new_with_data_dir_and_config(&fixed_path, operator_switch_join_config(false, 0))
                .unwrap();
        let switched =
            Cassie::new_with_data_dir_and_config(&switched_path, operator_switch_join_config(true, 1))
                .unwrap();
        let fixed_session = fixed.create_session("tester", None);
        let switched_session = switched.create_session("tester", None);
        for (cassie, session, users, orders) in [
            (
                &fixed,
                &fixed_session,
                "join_switch_fixed_users",
                "join_switch_fixed_orders",
            ),
            (
                &switched,
                &switched_session,
                "join_switch_replay_users",
                "join_switch_replay_orders",
            ),
        ] {
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE TABLE {users} (user_key INT, name TEXT)"),
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE TABLE {orders} (order_user_key INT, total INT)"),
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "INSERT INTO {users} (user_key, name) VALUES (1, 'ada'), (2, 'grace')"
                    ),
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "INSERT INTO {orders} (order_user_key, total) VALUES (1, 10), (2, 20)"
                    ),
                    vec![],
                )
                .unwrap();
        }

        // Act
        let fixed_result = fixed
            .execute_sql(
                &fixed_session,
                "SELECT join_switch_fixed_users.name, join_switch_fixed_orders.total FROM join_switch_fixed_users JOIN join_switch_fixed_orders ON join_switch_fixed_users.user_key = join_switch_fixed_orders.order_user_key ORDER BY join_switch_fixed_users.name",
                vec![],
            )
            .unwrap();
        let switched_result = switched
            .execute_sql(
                &switched_session,
                "SELECT join_switch_replay_users.name, join_switch_replay_orders.total FROM join_switch_replay_users JOIN join_switch_replay_orders ON join_switch_replay_users.user_key = join_switch_replay_orders.order_user_key ORDER BY join_switch_replay_users.name",
                vec![],
            )
            .unwrap();
        let metrics = switched.metrics();

        // Assert
        assert_eq!(fixed_result.rows, switched_result.rows);
        assert_eq!(
            switched_result.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(10)],
                vec![Value::String("grace".to_string()), Value::Int64(20)],
            ]
        );
        assert_eq!(
            metrics["adaptive_candidates"]["last_operator_switch_state"],
            "replay_left_rows=2;replay_right_rows=2;rows_emitted=0"
        );

        let _ = std::fs::remove_dir_all(fixed_path);
        let _ = std::fs::remove_dir_all(switched_path);
    });
    }

    #[test]
    fn should_execute_right_join_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_right");
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
                "CREATE TABLE right_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE right_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO right_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT right_users.name, right_orders.total FROM right_users RIGHT JOIN right_orders ON right_users.user_key = right_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::Null, Value::Int64(42)]]);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_full_outer_join_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_full_outer");
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
                "CREATE TABLE full_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE full_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO full_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO full_orders (order_user_key, total) VALUES (2, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT full_users.name, full_orders.total FROM full_users FULL OUTER JOIN full_orders ON full_users.user_key = full_orders.order_user_key ORDER BY full_users.name NULLS LAST",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Null],
                vec![Value::Null, Value::Int64(42)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_cross_join_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_cross");
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
                "CREATE TABLE cross_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cross_orders (order_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_users (user_key, name) VALUES (2, 'grace')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_orders (order_key, total) VALUES (10, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT cross_users.name, cross_orders.total FROM cross_users CROSS JOIN cross_orders ORDER BY cross_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Int64(42)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_lateral_join_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_lateral");
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
                "CREATE TABLE lateral_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE lateral_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_users (user_key, name) VALUES (2, 'grace')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (1, 99)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (2, 7)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT lateral_users.name, recent.total FROM lateral_users JOIN LATERAL (SELECT total FROM lateral_orders WHERE order_user_key = lateral_users.user_key ORDER BY total DESC LIMIT 1) AS recent ON true ORDER BY lateral_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(99)],
                vec![Value::String("grace".to_string()), Value::Int64(7)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_cross_apply_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_cross_apply");
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
                "CREATE TABLE apply_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_orders (order_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO apply_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO apply_orders (order_key, total) VALUES (10, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT apply_users.name, recent.total FROM apply_users CROSS APPLY (SELECT total FROM apply_orders) AS recent ORDER BY apply_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("ada".to_string()),
                Value::Int64(42)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_outer_apply_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("join_outer_apply");
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
                "CREATE TABLE outer_apply_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_missing_orders (order_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO outer_apply_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT outer_apply_users.name, recent.total FROM outer_apply_users OUTER APPLY (SELECT total FROM apply_missing_orders) AS recent ORDER BY outer_apply_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Null]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_from_subquery_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("from_subquery");
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
                    "CREATE TABLE from_subquery_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO from_subquery_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT recent.title FROM (SELECT title FROM from_subquery_docs) AS recent",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_null_semantics.rs.
mod integration_sql_null_semantics {
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_preserve_unknown_across_null_predicate_boolean_logic() {
        // Arrange
        use_local_storage();
        let path = data_dir("null_semantics");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE null_semantics (row_key INT, score INT, title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO null_semantics (row_key, score, title) VALUES (1, NULL, NULL), (2, 0, 'alpha'), (3, 1, 'beta')",
            vec![],
        )
        .expect("seed rows");

        // Act
        let selected_ids = |predicate: &str| {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "SELECT row_key FROM null_semantics WHERE {predicate} ORDER BY row_key"
                    ),
                    vec![],
                )
                .expect("evaluate null predicate")
                .rows
                .into_iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>()
        };

        // Assert
        assert_eq!(selected_ids("score = NULL"), Vec::<Value>::new());
        assert_eq!(selected_ids("(score + 1) = 1"), vec![Value::Int64(2)]);
        assert_eq!(
            selected_ids("score BETWEEN -1 AND 1"),
            vec![Value::Int64(2), Value::Int64(3)]
        );
        assert_eq!(selected_ids("score IN (1, NULL)"), vec![Value::Int64(3)]);
        assert_eq!(selected_ids("score NOT IN (1, NULL)"), Vec::<Value>::new());
        assert_eq!(selected_ids("NOT (score = 1)"), vec![Value::Int64(2)]);
        assert_eq!(
            selected_ids("NOT (score = 1 AND score <> 1)"),
            vec![Value::Int64(2), Value::Int64(3)]
        );
        assert_eq!(
            selected_ids("score = 1 OR score <> 1"),
            vec![Value::Int64(2), Value::Int64(3)]
        );
        assert_eq!(selected_ids("NOT (title LIKE 'a%')"), vec![Value::Int64(3)]);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_incompatible_operands_with_division_by_zero() {
        // Arrange
        use_local_storage();
        let path = data_dir("null_semantics_errors");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE null_semantics_errors (score INT, title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO null_semantics_errors (score, title) VALUES (4, 'four')",
                vec![],
            )
            .expect("seed row");

        // Act
        let incompatible = cassie.execute_sql(
            &session,
            "SELECT score FROM null_semantics_errors WHERE score = title",
            vec![],
        );
        let division = cassie.execute_sql(
            &session,
            "SELECT score FROM null_semantics_errors WHERE (score / 0) = 1",
            vec![],
        );

        // Assert
        assert!(incompatible
            .expect_err("incompatible comparison should fail")
            .to_string()
            .contains("incompatible"));
        assert_eq!(
            division
                .expect_err("division by zero should fail")
                .to_string(),
            "execution error: division by zero"
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_ordering.rs.
mod integration_sql_ordering {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_apply_limit_offset_after_ordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("limit_offset_order");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();

            let collection = "sql_limit_offset_order";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "pear", "body": "c"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"title": "apple", "body": "a"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"title": "banana", "body": "b"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_limit_offset_order ORDER BY title ASC LIMIT 2 OFFSET 1",
                vec![],
            )
            .unwrap();

            // Assert
            assert_eq!(result.columns.len(), 2);
            assert_eq!(result.columns[0].name, "id");
            assert_eq!(result.columns[1].name, "title");
            assert_eq!(result.rows.len(), 2);

            let rows = result.rows;
            let ids = rows
                .iter()
                .map(|row| match &row[0] {
                    cassie::types::Value::String(id) => id.clone(),
                    _ => panic!("expected string id"),
                })
                .collect::<Vec<_>>();
            assert_eq!(ids, vec!["d3".to_string(), "d1".to_string()]);

            let titles = rows
                .iter()
                .map(|row| match &row[1] {
                    cassie::types::Value::String(title) => title.clone(),
                    _ => panic!("expected string title"),
                })
                .collect::<Vec<_>>();
            assert_eq!(titles, vec!["banana".to_string(), "pear".to_string()]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_page_by_row_id_with_storage_top_k() {
        // Arrange
        use_local_storage();
        let path = data_dir("row_id_storage_top_k");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_row_id_storage_top_k";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            for (id, title) in [("d3", "three"), ("d1", "one"), ("d2", "two")] {
                cassie
                    .midge
                    .put_document(
                        collection,
                        Some(id.to_string()),
                        serde_json::json!({"title": title}),
                    )
                    .unwrap();
            }

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id, title FROM sql_row_id_storage_top_k ORDER BY id ASC LIMIT 2",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
            assert_eq!(result.rows[0][1], Value::String("one".to_string()));
            assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
            assert_eq!(result.rows[1][1], Value::String("two".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_page_by_row_id_with_keyset_cursor() {
        // Arrange
        use_local_storage();
        let path = data_dir("row_id_keyset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_row_id_keyset";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            for (id, title) in [("d1", "one"), ("d2", "two"), ("d3", "three")] {
                cassie
                    .midge
                    .put_document(
                        collection,
                        Some(id.to_string()),
                        serde_json::json!({"title": title}),
                    )
                    .unwrap();
            }

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_row_id_keyset WHERE id > 'd1' ORDER BY id ASC LIMIT 2",
                vec![],
            )
            .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
            assert_eq!(result.rows[0][1], Value::String("two".to_string()));
            assert_eq!(result.rows[1][0], Value::String("d3".to_string()));
            assert_eq!(result.rows[1][1], Value::String("three".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_projected_scan_range_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("projected_scan_range");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_projected_scan_range";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "low", "score": 1}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"title": "mid", "score": 10}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"title": "high", "score": 20}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id, title FROM sql_projected_scan_range WHERE score >= 10 LIMIT 2",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
            assert_eq!(result.rows[0][1], Value::String("mid".to_string()));
            assert_eq!(result.rows[1][0], Value::String("d3".to_string()));
            assert_eq!(result.rows[1][1], Value::String("high".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_projected_scan_simple_equality_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("projected_scan_equality");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_projected_scan_equality";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "alpha", "body": "first"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"title": "beta", "body": "second"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id, title FROM sql_projected_scan_equality WHERE title = 'beta'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
            assert_eq!(result.rows[0][1], Value::String("beta".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_apply_vector_distance_offset_after_ordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_distance_offset_order");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_vector_distance_offset_order";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [3.0, 0.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM sql_vector_distance_offset_order ORDER BY distance ASC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(1.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_fulltext_offset_after_score_ordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_top_k_offset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_top_k_offset";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "alpha alpha"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_top_k_offset WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_hybrid_offset_after_score_ordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("hybrid_top_k_offset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_offset";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "red red", "embedding": [2.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_top_k_offset ORDER BY score DESC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_order_timestamps_with_mixed_utc_offsets_by_instant() {
        // Arrange
        use_local_storage();
        let path = data_dir("timestamp_mixed_offset_order");
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
                    "CREATE TABLE timestamp_mixed_offset_order (seq INT, ts TIMESTAMP)",
                    vec![],
                )
                .unwrap();
            // Row 1 is 2024-01-01T07:00:00Z once its +02:00 offset is applied,
            // an hour before row 2's plain UTC instant.
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO timestamp_mixed_offset_order (seq, ts) VALUES (1, '2024-01-01T09:00:00+02:00')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO timestamp_mixed_offset_order (seq, ts) VALUES (2, '2024-01-01T08:00:00Z')",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT seq FROM timestamp_mixed_offset_order ORDER BY ts ASC",
                    vec![],
                )
                .unwrap();

            // Assert: row 1's instant (07:00Z) is earlier than row 2's (08:00Z).
            assert_eq!(
                result.rows,
                vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_predicates.rs.
mod integration_sql_predicates {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_filter_rows_with_is_null_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_is_null");
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
                    "CREATE TABLE predicate_is_null (title TEXT, archived_at TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_is_null (title, archived_at) VALUES ('alpha', NULL)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_is_null (title, archived_at) VALUES ('beta', 'today')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_is_null WHERE archived_at IS NULL",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_in_list_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_in_list");
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
                    "CREATE TABLE predicate_in_list (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_in_list (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_in_list (title) VALUES ('gamma')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_in_list WHERE title IN ('alpha', 'beta')",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_not_in_list_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_not_in_list");
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
                    "CREATE TABLE predicate_not_in_list (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_not_in_list (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_not_in_list (title) VALUES ('gamma')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_not_in_list WHERE title NOT IN ('alpha', 'beta')",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("gamma".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_between_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_between");
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
                    "CREATE TABLE predicate_between (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_between (title, score) VALUES ('alpha', 5)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_between (title, score) VALUES ('beta', 15)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_between WHERE score BETWEEN 10 AND 20",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_not_between_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_not_between");
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
                    "CREATE TABLE predicate_not_between (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_not_between (title, score) VALUES ('alpha', 5)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_not_between (title, score) VALUES ('beta', 15)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_not_between WHERE score NOT BETWEEN 10 AND 20",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_order_nulls_first_when_requested() {
        // Arrange
        use_local_storage();
        let path = data_dir("order_nulls_first");
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
                    "CREATE TABLE order_nulls_first (title TEXT, archived_at TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO order_nulls_first (title, archived_at) VALUES ('alpha', 'today')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO order_nulls_first (title, archived_at) VALUES ('beta', NULL)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM order_nulls_first ORDER BY archived_at NULLS FIRST",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("beta".to_string())],
                    vec![Value::String("alpha".to_string())],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_order_nulls_last_when_requested() {
        // Arrange
        use_local_storage();
        let path = data_dir("order_nulls_last");
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
                    "CREATE TABLE order_nulls_last (title TEXT, archived_at TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO order_nulls_last (title, archived_at) VALUES ('alpha', 'today')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO order_nulls_last (title, archived_at) VALUES ('beta', NULL)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM order_nulls_last ORDER BY archived_at NULLS LAST",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("alpha".to_string())],
                    vec![Value::String("beta".to_string())],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_exists_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_exists");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_outer (title TEXT)", vec![])

.unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_inner (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_inner (title) VALUES ('present')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_exists_outer WHERE EXISTS (SELECT title FROM predicate_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_filter_rows_with_empty_exists_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_empty_exists");
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
                "CREATE TABLE predicate_empty_exists_outer (title TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_inner (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_empty_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_empty_exists_outer WHERE EXISTS (SELECT title FROM predicate_empty_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_filter_rows_with_not_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_not");
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
                    "CREATE TABLE predicate_not_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_not_docs (title) VALUES ('keep')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_not_docs (title) VALUES ('skip')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_not_docs WHERE NOT title = 'skip'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::String("keep".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_not_exists_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_not_exists");
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
                "CREATE TABLE predicate_not_exists_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_exists_inner (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_exists_outer WHERE NOT EXISTS (SELECT title FROM predicate_not_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_filter_rows_with_is_not_null_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_is_not_null");
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
                    "CREATE TABLE predicate_is_not_null (title TEXT, archived_at TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('alpha', NULL)",
                    vec![],
                )
                .unwrap();
            cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_is_not_null WHERE archived_at IS NOT NULL",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_match_like_wildcards_anywhere_in_pattern() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_like_wildcards");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_like_wildcards (row_key INT, name TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_like_wildcards (row_key, name) VALUES (1, 'foobar'), (2, 'abc'), (3, 'ABC'), (4, 'a%c'), (5, 'a_c'), (6, 'axxc'), (7, 'fooxbarbar'), (8, NULL)",
                vec![],
            )
            .expect("seed rows");
        let selected_ids = |predicate: &str, params: Vec<Value>| {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "SELECT row_key FROM predicate_like_wildcards WHERE {predicate} ORDER BY row_key"
                    ),
                    params,
                )
                .expect("evaluate like predicate")
                .rows
                .into_iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>()
        };
        let ids = |values: &[i64]| values.iter().copied().map(Value::Int64).collect::<Vec<_>>();

        // Act
        let interior = selected_ids("name LIKE 'foo%bar'", vec![]);
        let multiple = selected_ids("name LIKE '%a%c%'", vec![]);
        let single = selected_ids("name LIKE 'a_c'", vec![]);
        let repeated = selected_ids("name LIKE 'foo%%bar'", vec![]);
        let escaped_percent = selected_ids("name LIKE 'a\\%c'", vec![]);
        let escaped_underscore = selected_ids("name LIKE 'a\\_c'", vec![]);
        let case_sensitive = selected_ids("name LIKE 'A%'", vec![]);
        let parameterized = selected_ids("name LIKE $1", vec![Value::String("%o%b_r".to_string())]);
        let negated = selected_ids("NOT (name LIKE '%a%')", vec![]);

        // Assert
        assert_eq!(interior, ids(&[1, 7]), "foo%bar");
        assert_eq!(multiple, ids(&[2, 4, 5, 6]), "%a%c%");
        assert_eq!(single, ids(&[2, 4, 5]), "a_c");
        assert_eq!(repeated, ids(&[1, 7]), "foo%%bar");
        assert_eq!(escaped_percent, ids(&[4]), "escaped percent");
        assert_eq!(escaped_underscore, ids(&[5]), "escaped underscore");
        assert_eq!(case_sensitive, ids(&[3]), "LIKE is case-sensitive");
        assert_eq!(parameterized, ids(&[1, 7]), "parameterized pattern");
        assert_eq!(negated, ids(&[3]), "negated LIKE keeps NULL out");

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_projection.rs.
mod integration_sql_projection {
    #![allow(unused_imports, dead_code)]
    use cassie::app::{Cassie, CassieSession};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::runtime::RuntimeFeedbackObservation;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    fn seed_column_store_projection_crud(cassie: &Cassie, session: &CassieSession) {
        for sql in [
        "CREATE TABLE sql_column_store_projection_crud (doc_id TEXT, title TEXT, summary TEXT, score INT) WITH (storage = column_store)",
        "INSERT INTO sql_column_store_projection_crud (doc_id, title, summary, score) VALUES ('d1', 'alpha', NULL, 10)",
        "INSERT INTO sql_column_store_projection_crud (doc_id, title, score) VALUES ('d2', 'beta', 20)",
    ] {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }
    }

    fn assert_column_store_before_rows(result: &cassie::executor::QueryResult) {
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("d1".to_string()),
                    Value::String("alpha".to_string()),
                    Value::Null,
                    Value::Int64(10),
                ],
                vec![
                    Value::String("d2".to_string()),
                    Value::String("beta".to_string()),
                    Value::Null,
                    Value::Int64(20),
                ],
            ]
        );
    }

    fn assert_column_store_after_rows(result: &cassie::executor::QueryResult) {
        assert_eq!(
            result.rows,
            vec![vec![
                Value::String("d2".to_string()),
                Value::String("beta".to_string()),
                Value::String("filled".to_string()),
                Value::Int64(20),
            ]]
        );
    }

    fn adaptive_execution_config() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::default();
        config.limits.operator_feedback_enabled = true;
        config.limits.adaptive_execution_enabled = true;
        config.limits.adaptive_min_cost_savings_bps = 0;
        config
    }

    fn confident_feedback(elapsed_ms: u64, storage_reads: u64) -> RuntimeFeedbackObservation {
        RuntimeFeedbackObservation {
            rows_in: storage_reads.max(1),
            rows_out: 1,
            elapsed_ms,
            storage_reads,
            ..RuntimeFeedbackObservation::default()
        }
    }

    #[test]
    fn should_order_column_top_k_with_deterministic_tie_break() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_top_k_tie");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();

            let collection = "sql_column_top_k_tie";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"title": "second", "score": 10}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "first", "score": 10}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"title": "third", "score": 1}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM sql_column_top_k_tie ORDER BY score DESC LIMIT 2",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
            assert_eq!(result.rows[1][0], Value::String("d2".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_results_for_adaptive_read_operator_choice() {
        // Arrange
        use_local_storage();
        let fixed_path = data_dir("adaptive_read_operator_fixed");
        let adaptive_path = data_dir("adaptive_read_operator_enabled");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let fixed = Cassie::new_with_data_dir(&fixed_path).unwrap();
        let adaptive =
            Cassie::new_with_data_dir_and_config(&adaptive_path, adaptive_execution_config())
                .unwrap();
        let fixed_session = fixed.create_session("tester", None);
        let adaptive_session = adaptive.create_session("tester", None);
        for cassie in [&fixed, &adaptive] {
            let session = if std::ptr::eq(cassie, &raw const fixed) {
                &fixed_session
            } else {
                &adaptive_session
            };
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE sql_adaptive_projection (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    "CREATE INDEX sql_adaptive_projection_body_idx_a ON sql_adaptive_projection (body)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    "CREATE INDEX sql_adaptive_projection_title_idx_b ON sql_adaptive_projection (title)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO sql_adaptive_projection (title, body) VALUES ('alpha', 'one'), ('beta', 'two')",
                    vec![],
                )
                .unwrap();
        }

        let sql = "SELECT title FROM sql_adaptive_projection WHERE title = 'alpha' AND body = 'one'";
        let base_index = "sql_adaptive_projection_body_idx_a";
        let preferred_index = "sql_adaptive_projection_title_idx_b";
        let base_key = adaptive
            .read_operator_feedback_key_for_diagnostics(&adaptive_session, sql, Some(base_index))
            .unwrap();
        let preferred_key = adaptive
            .read_operator_feedback_key_for_diagnostics(
                &adaptive_session,
                sql,
                Some(preferred_index),
            )
            .unwrap();
        for _ in 0..4 {
            adaptive
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .unwrap();
            adaptive
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .unwrap();
        }

        // Act
        let fixed_result = fixed.execute_sql(&fixed_session, sql, vec![]).unwrap();
        let adaptive_result = adaptive
            .execute_sql(&adaptive_session, sql, vec![])
            .unwrap();
        let adaptive_plan = adaptive
            .execute_sql(&adaptive_session, &format!("EXPLAIN {sql}"), vec![])
            .unwrap();
        let plan = adaptive_plan.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert_eq!(fixed_result.rows, adaptive_result.rows);
        assert_eq!(
            adaptive_result.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );
        assert!(plan.contains(preferred_index), "plan={plan}");
        assert!(
            plan.contains("adaptive_reason=selected_operator_feedback"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(fixed_path);
        let _ = std::fs::remove_dir_all(adaptive_path);
    });
    }

    #[test]
    fn should_fall_back_for_filtered_ordered_column_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_top_k_filter_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_column_top_k_filter_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "skip", "score": 100}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "keep", "score": 10}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM sql_column_top_k_filter_fallback WHERE title = 'keep' ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fall_back_for_function_projection_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("projected_scan_function_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_function_fallback";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT upper(title) FROM sql_projected_scan_function_fallback WHERE title = 'alpha'",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("ALPHA".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fall_back_for_wildcard_projection_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("projected_scan_wildcard_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_projected_scan_wildcard_fallback";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "alpha", "score": 7}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT * FROM sql_projected_scan_wildcard_fallback WHERE score = 7",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
            assert_eq!(result.rows[0][1], Value::String("alpha".to_string()));
            assert_eq!(result.rows[0][2], Value::Int64(7));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_describe_select_projection_with_column_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("describe_sql_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_describe_metadata";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            // Act
            let columns = cassie
                .describe_sql("SELECT id, title, score FROM sql_describe_metadata")
                .unwrap();

            // Assert
            assert_eq!(columns.len(), 3);
            assert_eq!(columns[0].name, "id");
            assert_eq!(columns[0].type_oid, DataType::Text.type_oid());
            assert_eq!(columns[1].name, "title");
            assert_eq!(columns[1].data_type, "text");
            assert_eq!(columns[2].name, "score");
            assert_eq!(columns[2].type_oid, DataType::Int.type_oid());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_projected_crud_queries_against_column_store_tables() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_store_projection_crud");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let collection = "sql_column_store_projection_crud";

        seed_column_store_projection_crud(&cassie, &session);
        let canonical_collection = canonical_test_collection(&cassie, collection);

        // Act
        let before = cassie
            .execute_sql(
                &session,
                "SELECT doc_id, title, summary, score FROM sql_column_store_projection_crud ORDER BY doc_id",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, score FROM sql_column_store_projection_crud WHERE doc_id = 'd2'",
                vec![],
            )
            .unwrap();
        let documents = cassie.midge.scan_documents(&canonical_collection).unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE sql_column_store_projection_crud SET summary = 'filled' WHERE doc_id = 'd2'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM sql_column_store_projection_crud WHERE doc_id = 'd1'",
                vec![],
            )
            .unwrap();
        let after = cassie
            .execute_sql(
                &session,
                "SELECT doc_id, title, summary, score FROM sql_column_store_projection_crud ORDER BY doc_id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_column_store_before_rows(&before);
        let explicit_null = documents
            .iter()
            .find(|document| {
                document.payload.get("doc_id") == Some(&serde_json::Value::String("d1".to_string()))
            })
            .expect("stored null row");
        let missing = documents
            .iter()
            .find(|document| {
                document.payload.get("doc_id") == Some(&serde_json::Value::String("d2".to_string()))
            })
            .expect("stored missing row");
        assert!(matches!(
            explicit_null.payload.get("summary"),
            Some(serde_json::Value::Null)
        ));
        assert!(missing.payload.get("summary").is_none());

        let plan = explain_plan_text(&explain);
        assert_explain_contains(plan, "storage_mode", "column-store");

        assert_column_store_after_rows(&after);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_create_column_store_tables_without_feature_gate() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_store_disabled");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_column_store_enabled (title TEXT) WITH (storage = column_store)",
                vec![],
            )
            .expect("column-store creation should not require a feature gate");

            // Assert
            assert_eq!(result.command, "CREATE TABLE");
            assert!(cassie.catalog.exists("sql_column_store_enabled"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_column_store_schema_rewrites_before_partial_write() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_store_schema_rewrite");
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
                "CREATE TABLE sql_column_store_schema_rewrite (title TEXT) WITH (storage = column_store)",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "ALTER TABLE sql_column_store_schema_rewrite ADD COLUMN summary TEXT",
                vec![],
            )
            .expect_err("column-store schema rewrite should fail");
        let columns = cassie.describe_sql("SELECT title FROM sql_column_store_schema_rewrite").unwrap();

        // Assert
        assert!(error.to_string().contains("column-store"));
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].name, "title");

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_read_path_depth.rs.
mod integration_sql_read_path_depth {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_scan_mixed_order_suffix_when_prefix_order_field_is_equality_bound() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_mixed_order_prefix");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_path_mixed_order_prefix \
                 (tenant TEXT, status TEXT, created_at INT, title TEXT)",
                    vec![],
                )
                .unwrap();
            for (id, tenant, status, created_at, title) in [
                ("row-1", "tenant-a", "open", 30, "third"),
                ("row-2", "tenant-a", "open", 10, "first"),
                ("row-3", "tenant-a", "open", 20, "second"),
                ("row-4", "tenant-a", "closed", 5, "closed"),
                ("row-5", "tenant-b", "open", 1, "other"),
            ] {
                cassie
                    .midge
                    .put_document(
                        "read_path_mixed_order_prefix",
                        Some(id.to_string()),
                        serde_json::json!({
                            "tenant": tenant,
                            "status": status,
                            "created_at": created_at,
                            "title": title
                        }),
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_mixed_order_prefix_idx \
                 ON read_path_mixed_order_prefix USING btree (tenant, status, created_at)",
                    vec![],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM read_path_mixed_order_prefix \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM read_path_mixed_order_prefix \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("first".to_string())],
                    vec![Value::String("second".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=read_path_mixed_order_prefix_idx"));
            assert!(plan.contains("access_path=range_scan"));
            assert!(plan.contains("access_path_reason=scalar-index-range"));
            assert!(plan.contains("fallback_reason=none"));
            assert!(
                after["read_paths"]["range_scans"].as_u64().unwrap()
                    > before["read_paths"]["range_scans"].as_u64().unwrap()
            );
            assert_eq!(
                after["read_paths"]["last_index_scan_index"].as_str(),
                Some("read_path_mixed_order_prefix_idx")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_scan_index_prefix_then_sort_mixed_direction_suffix() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_mixed_order_suffix");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_path_mixed_order_suffix \
                 (tenant TEXT, created_at INT NOT NULL, priority INT NOT NULL, title TEXT)",
                    vec![],
                )
                .unwrap();
            for (id, tenant, created_at, priority, title) in [
                ("row-1", "tenant-a", 30, 5, "late-high"),
                ("row-2", "tenant-a", 30, 1, "late-low"),
                ("row-3", "tenant-a", 20, 1, "mid-low"),
                ("row-4", "tenant-a", 20, 9, "mid-high"),
                ("row-5", "tenant-b", 99, 1, "other"),
            ] {
                cassie
                    .midge
                    .put_document(
                        "read_path_mixed_order_suffix",
                        Some(id.to_string()),
                        serde_json::json!({
                            "tenant": tenant,
                            "created_at": created_at,
                            "priority": priority,
                            "title": title
                        }),
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_mixed_order_suffix_idx \
                 ON read_path_mixed_order_suffix USING btree (tenant, created_at, priority)",
                    vec![],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM read_path_mixed_order_suffix \
                 WHERE tenant = 'tenant-a' \
                 ORDER BY created_at DESC, priority ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM read_path_mixed_order_suffix \
                 WHERE tenant = 'tenant-a' \
                 ORDER BY created_at DESC, priority ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("late-low".to_string())],
                    vec![Value::String("late-high".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=read_path_mixed_order_suffix_idx"));
            assert!(plan.contains("access_path=prefix_scan"));
            assert!(plan.contains("access_path_reason=scalar-index-prefix"));
            assert!(plan.contains("fallback_reason=none"));
            assert!(plan.contains("top_k_mode=heap"));
            assert!(plan.contains("early_stop=none"));
            assert!(
                after["read_paths"]["prefix_scans"].as_u64().unwrap()
                    > before["read_paths"]["prefix_scans"].as_u64().unwrap()
            );
            assert!(
                after["read_paths"]["heap_top_k_scans"].as_u64().unwrap()
                    > before["read_paths"]["heap_top_k_scans"].as_u64().unwrap()
            );
            assert_eq!(
                after["read_paths"]["last_index_scan_index"].as_str(),
                Some("read_path_mixed_order_suffix_idx")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_scan_nonselective_index_prefix_then_heap_mixed_direction_suffix() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_nonselective_mixed_order_suffix");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_path_nonselective_mixed_order_suffix \
                 (score INT NOT NULL, title TEXT)",
                    vec![],
                )
                .unwrap();
            for (id, score, title) in [
                ("row-c", 100, "third"),
                ("row-a", 100, "first"),
                ("row-b", 100, "second"),
                ("row-z", 90, "lower"),
            ] {
                cassie
                    .midge
                    .put_document(
                        "read_path_nonselective_mixed_order_suffix",
                        Some(id.to_string()),
                        serde_json::json!({
                            "score": score,
                            "title": title
                        }),
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_nonselective_mixed_order_suffix_idx \
                 ON read_path_nonselective_mixed_order_suffix USING btree (score)",
                    vec![],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM read_path_nonselective_mixed_order_suffix \
                 ORDER BY score DESC, id ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT id FROM read_path_nonselective_mixed_order_suffix \
                 ORDER BY score DESC, id ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("row-a".to_string())],
                    vec![Value::String("row-b".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=read_path_nonselective_mixed_order_suffix_idx"));
            assert!(plan.contains("access_path=prefix_scan"));
            assert!(plan.contains("access_path_reason=scalar-index-prefix"));
            assert!(plan.contains("fallback_reason=none"));
            assert!(plan.contains("top_k_mode=heap"));
            assert!(plan.contains("early_stop=none"));
            assert!(
                after["read_paths"]["prefix_scans"].as_u64().unwrap()
                    > before["read_paths"]["prefix_scans"].as_u64().unwrap()
            );
            assert!(
                after["read_paths"]["heap_top_k_scans"].as_u64().unwrap()
                    > before["read_paths"]["heap_top_k_scans"].as_u64().unwrap()
            );
            assert_eq!(
                after["read_paths"]["last_index_scan_index"].as_str(),
                Some("read_path_nonselective_mixed_order_suffix_idx")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_scan_expression_index_after_restart_with_row_blob_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_expression_index_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_path_expression_index_restart (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = cassie
                .catalog
                .get_schema("read_path_expression_index_restart")
                .expect("catalog collection")
                .collection;
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("row-1".to_string()),
                    serde_json::json!({"title": "Alpha", "body": "kept in row blob"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("row-2".to_string()),
                    serde_json::json!({"title": "Beta", "body": "filtered"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_expression_index_restart_idx \
                     ON read_path_expression_index_restart USING btree (lower(title))",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let before = restarted.metrics();

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT body FROM read_path_expression_index_restart WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_path_expression_index_restart WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let after = restarted.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("kept in row blob".to_string())]]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(
            plan.contains("read_path_expression_index_restart_idx"),
            "plan={plan}"
        );
        assert!(plan.contains("access_path=index_seek"));
        assert!(plan.contains("access_path_reason=scalar-index-seek"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(
            after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
        );
        assert!(
            after["read_paths"]["last_index_scan_index"]
                .as_str()
                .is_some_and(|value| value.ends_with("read_path_expression_index_restart_idx"))
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_scan_expression_index_range_with_row_blob_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_expression_index_range");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_path_expression_index_range (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            for (id, title, body) in [
                ("row-1", "Alpha", "below"),
                ("row-2", "Omega", "inside"),
                ("row-3", "Zulu", "above"),
            ] {
                cassie
                    .midge
                    .put_document(
                        "read_path_expression_index_range",
                        Some(id.to_string()),
                        serde_json::json!({"title": title, "body": body}),
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_expression_index_range_idx \
                 ON read_path_expression_index_range USING btree (lower(title))",
                    vec![],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT body FROM read_path_expression_index_range \
                 WHERE lower(title) >= 'm' AND lower(title) < 'z'",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT body FROM read_path_expression_index_range \
                 WHERE lower(title) >= 'm' AND lower(title) < 'z'",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("inside".to_string())]]);
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=read_path_expression_index_range_idx"));
            assert!(plan.contains("access_path=range_scan"));
            assert!(plan.contains("access_path_reason=scalar-index-range"));
            assert!(plan.contains("fallback_reason=none"));
            assert!(
                after["read_paths"]["range_scans"].as_u64().unwrap()
                    > before["read_paths"]["range_scans"].as_u64().unwrap()
            );
            assert_eq!(
                after["read_paths"]["last_index_scan_index"].as_str(),
                Some("read_path_expression_index_range_idx")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    fn seed_nullable_expression_order_rows(cassie: &Cassie, session: &cassie::app::CassieSession) {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE read_path_expression_index_order_asc (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (id, payload) in [
            (
                "row-1",
                serde_json::json!({"title": "Beta", "body": "second"}),
            ),
            (
                "row-2",
                serde_json::json!({"title": "alpha", "body": "first"}),
            ),
            (
                "row-3",
                serde_json::json!({"title": "Gamma", "body": "third"}),
            ),
            (
                "row-4",
                serde_json::json!({"title": null, "body": "untitled"}),
            ),
        ] {
            cassie
                .midge
                .put_document(
                    "read_path_expression_index_order_asc",
                    Some(id.to_string()),
                    payload,
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                session,
                "CREATE INDEX read_path_expression_index_order_asc_idx \
         ON read_path_expression_index_order_asc USING btree (lower(title))",
                vec![],
            )
            .unwrap();
    }

    #[test]
    fn should_return_null_expression_keys_after_index_ordered_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_expression_index_order_asc");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            seed_nullable_expression_order_rows(&cassie, &session);
            let before = cassie.metrics();

            // Act
            let filled = cassie
                .execute_sql(
                    &session,
                    "SELECT body FROM read_path_expression_index_order_asc \
                 ORDER BY lower(title) ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let after_filled = cassie.metrics();
            let unfilled = cassie
                .execute_sql(
                    &session,
                    "SELECT body FROM read_path_expression_index_order_asc \
                 ORDER BY lower(title) ASC LIMIT 5",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT body FROM read_path_expression_index_order_asc \
                 ORDER BY lower(title) ASC LIMIT 2",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                filled.rows,
                vec![
                    vec![Value::String("first".to_string())],
                    vec![Value::String("second".to_string())],
                ]
            );
            assert_eq!(
                unfilled.rows,
                vec![
                    vec![Value::String("first".to_string())],
                    vec![Value::String("second".to_string())],
                    vec![Value::String("third".to_string())],
                    vec![Value::String("untitled".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=read_path_expression_index_order_asc_idx"));
            assert!(plan.contains("access_path=ordered_bounded_scan"));
            assert!(plan.contains("top_k_mode=storage"));
            assert!(
                after_filled["read_paths"]["ordered_bounded_scans"]
                    .as_u64()
                    .unwrap()
                    > before["read_paths"]["ordered_bounded_scans"]
                        .as_u64()
                        .unwrap()
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_decline_expression_index_order_limit_when_null_keys_sort_first() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_expression_index_order");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_path_expression_index_order (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            for (id, payload) in [
                (
                    "row-1",
                    serde_json::json!({"title": "Beta", "body": "second"}),
                ),
                (
                    "row-2",
                    serde_json::json!({"title": "alpha", "body": "third"}),
                ),
                (
                    "row-3",
                    serde_json::json!({"title": "Gamma", "body": "first"}),
                ),
                (
                    "row-4",
                    serde_json::json!({"title": null, "body": "untitled"}),
                ),
            ] {
                cassie
                    .midge
                    .put_document(
                        "read_path_expression_index_order",
                        Some(id.to_string()),
                        payload,
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_expression_index_order_idx \
                 ON read_path_expression_index_order USING btree (lower(title))",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT body FROM read_path_expression_index_order \
                 ORDER BY lower(title) DESC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT body FROM read_path_expression_index_order \
                 ORDER BY lower(title) DESC LIMIT 2",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("untitled".to_string())],
                    vec![Value::String("first".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(!plan.contains("access_path=ordered_bounded_scan"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_scalar_functions.rs.
mod integration_sql_scalar_functions {
    #![allow(unused_imports, dead_code)]

    use super::support_sql as support;
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use support::*;

    // should_execute_text_scalar_functions_query removed: strictly subsumed by
    // scalar_functions.rs::should_execute_string_scalar_functions_in_query_path,
    // which covers the same lower/upper/trim/substring/concat/length assertions
    // plus len()/WHERE/ORDER BY on the same operations.

    #[test]
    fn should_execute_coalesce_scalar_function_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_coalesce_function");
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
                    "CREATE TABLE scalar_coalesce_function (title TEXT, fallback TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO scalar_coalesce_function (title, fallback) VALUES (NULL, 'backup')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT coalesce(title, fallback, 'missing') AS value FROM scalar_coalesce_function",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("backup".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_numeric_scalar_function_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_numeric_function");
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
                    "CREATE TABLE scalar_numeric_function (delta INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO scalar_numeric_function (delta) VALUES (-42)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT abs(delta) AS magnitude FROM scalar_numeric_function",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::Int64(42)]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_abs_overflow_for_bigint_minimum() {
        // Arrange
        use_local_storage();
        let path = data_dir("abs_bigint_overflow");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let overflow = cassie.execute_sql(
                &session,
                "SELECT ABS(CAST('-9223372036854775808' AS BIGINT)) AS result",
                vec![],
            );

            // Assert
            assert!(overflow
                .expect_err("reject an unrepresentable BIGINT magnitude")
                .to_string()
                .contains("integer overflow"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_abs_at_numeric_boundaries() {
        // Arrange
        use_local_storage();
        let path = data_dir("abs_numeric_boundaries");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let adjacent = cassie
                .execute_sql(
                    &session,
                    "SELECT ABS(CAST('-9223372036854775807' AS BIGINT)), ABS(CAST('9223372036854775807' AS BIGINT)), ABS(CAST('-1.5' AS FLOAT))",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                adjacent.rows,
                vec![vec![
                    Value::Int64(i64::MAX),
                    Value::Int64(i64::MAX),
                    Value::Float64(1.5),
                ]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_cast_function_expression() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_cast_function");
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
                    "CREATE TABLE predicate_cast_function (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_cast_function (title, score) VALUES ('alpha', 10)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_cast_function WHERE CAST(score AS TEXT) = '10'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_rows_with_postgres_style_cast_expression() {
        // Arrange
        use_local_storage();
        let path = data_dir("predicate_pg_cast");
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
                    "CREATE TABLE predicate_pg_cast (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO predicate_pg_cast (title, score) VALUES ('alpha', 10)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM predicate_pg_cast WHERE score::TEXT = '10'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_project_rows_with_cast_expressions() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_cast_expressions");
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
                    "CREATE TABLE projection_cast_expressions (score INT, active BOOLEAN, flag TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO projection_cast_expressions (score, active, flag) VALUES (10, true, 't')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT CAST(score AS TEXT) AS score_text, score::FLOAT AS score_float, CAST(active AS INT) AS active_int, CAST(flag AS BOOLEAN) AS flag_bool FROM projection_cast_expressions",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![
                    Value::String("10".to_string()),
                    Value::Float64(10.0),
                    Value::Int64(1),
                    Value::Bool(true)
                ]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_invalid_cast_expression() {
        // Arrange
        use_local_storage();
        let path = data_dir("invalid_cast_expression");
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
                    "CREATE TABLE invalid_cast_expression (label TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO invalid_cast_expression (label) VALUES ('not-a-number')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie.execute_sql(
                &session,
                "SELECT CAST(label AS INT) FROM invalid_cast_expression",
                vec![],
            );

            // Assert
            assert!(selected.is_err());
            assert!(selected
                .unwrap_err()
                .to_string()
                .contains("cannot cast value to INT"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_case_expressions_with_a_clear_error() {
        // Arrange
        use_local_storage();
        let path = data_dir("reject_case_expression");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);

        // Act
        let selected = cassie.execute_sql(
            &session,
            "SELECT CASE WHEN 1 = 1 THEN 'a' ELSE 'b' END",
            vec![],
        );

        // Assert: CASE is unimplemented, so it must fail cleanly rather than
        // being misparsed as a call to a function literally named
        // "CASE WHEN...".
        let message = selected.unwrap_err().to_string();
        assert!(
            message.contains("CASE expressions are not supported"),
            "{message}"
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
// Formerly tests/integration_sql_sets.rs.
mod integration_sql_sets {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_distinct_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("distinct_query");
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
                    "CREATE TABLE distinct_docs (category TEXT)",
                    vec![],
                )
                .unwrap();
            for sql in [
                "INSERT INTO distinct_docs (category) VALUES ('b')",
                "INSERT INTO distinct_docs (category) VALUES ('a')",
                "INSERT INTO distinct_docs (category) VALUES ('a')",
            ] {
                cassie.execute_sql(&session, sql, vec![]).unwrap();
            }

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT DISTINCT category FROM distinct_docs ORDER BY category",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("a".to_string())],
                    vec![Value::String("b".to_string())]
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_union_all_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("union_all_query");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE TABLE union_all_left (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE union_all_right (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO union_all_left (title) VALUES ('beta')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO union_all_right (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO union_all_right (title) VALUES ('beta')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM union_all_left UNION ALL SELECT title FROM union_all_right",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("alpha".to_string())],
                    vec![Value::String("beta".to_string())],
                    vec![Value::String("beta".to_string())]
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_union_query_with_deduplication() {
        // Arrange
        use_local_storage();
        let path = data_dir("union_query");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE TABLE union_left (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(&session, "CREATE TABLE union_right (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO union_left (title) VALUES ('beta')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO union_right (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO union_right (title) VALUES ('beta')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM union_left UNION SELECT title FROM union_right",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("alpha".to_string())],
                    vec![Value::String("beta".to_string())]
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_intersect_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("intersect_query");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE TABLE intersect_left (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE intersect_right (title TEXT)",
                    vec![],
                )
                .unwrap();
            for sql in [
                "INSERT INTO intersect_left (title) VALUES ('alpha')",
                "INSERT INTO intersect_left (title) VALUES ('beta')",
                "INSERT INTO intersect_right (title) VALUES ('beta')",
                "INSERT INTO intersect_right (title) VALUES ('gamma')",
            ] {
                cassie.execute_sql(&session, sql, vec![]).unwrap();
            }

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM intersect_left INTERSECT SELECT title FROM intersect_right",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_except_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("except_query");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE TABLE except_left (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(&session, "CREATE TABLE except_right (title TEXT)", vec![])
                .unwrap();
            for sql in [
                "INSERT INTO except_left (title) VALUES ('alpha')",
                "INSERT INTO except_left (title) VALUES ('beta')",
                "INSERT INTO except_right (title) VALUES ('beta')",
                "INSERT INTO except_right (title) VALUES ('gamma')",
            ] {
                cassie.execute_sql(&session, sql, vec![]).unwrap();
            }

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM except_left EXCEPT SELECT title FROM except_right",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_distinct_on_query_with_ordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("distinct_on_query");
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
                "CREATE TABLE distinct_on_docs (tenant_id TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'low', 1)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'high', 9)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('b', 'only', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT ON (tenant_id) tenant_id, title FROM distinct_on_docs ORDER BY tenant_id ASC, score DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("a".to_string()),
                    Value::String("high".to_string())
                ],
                vec![
                    Value::String("b".to_string()),
                    Value::String("only".to_string())
                ]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_order_limit_offset_after_union_all() {
        // Arrange
        use_local_storage();
        let path = data_dir("union_global_order");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_right (title TEXT)", vec![])
            .unwrap();
        for sql in [
            "INSERT INTO union_order_left (title) VALUES ('beta')",
            "INSERT INTO union_order_right (title) VALUES ('alpha')",
            "INSERT INTO union_order_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_order_left UNION ALL SELECT title FROM union_order_right ORDER BY title LIMIT 1 OFFSET 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_chained_union_all_query() {
        // Arrange
        use_local_storage();
        let path = data_dir("union_all_chained");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for table in ["union_chain_a", "union_chain_b", "union_chain_c"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {table} (title TEXT)"), vec![])
                .unwrap();
        }
        for sql in [
            "INSERT INTO union_chain_a (title) VALUES ('alpha')",
            "INSERT INTO union_chain_b (title) VALUES ('beta')",
            "INSERT INTO union_chain_c (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_chain_a UNION ALL SELECT title FROM union_chain_b UNION ALL SELECT title FROM union_chain_c ORDER BY title DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("gamma".to_string())],
                vec![Value::String("beta".to_string())],
                vec![Value::String("alpha".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_table_free.rs.
mod integration_sql_table_free {
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_execute_table_free_literal_alias_cast_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("table_free_literals");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT 1 AS one, 'alpha' AS label, true AS enabled, NULL AS missing, CAST(2 AS INT) AS two, 1 + 2 AS total",
            vec![],
        )
        .expect("execute table-free projection");

        // Assert
        assert_eq!(
            result
                .columns
                .iter()
                .map(|column| (column.name.as_str(), column.type_oid))
                .collect::<Vec<_>>(),
            vec![
                ("one", 701),
                ("label", 25),
                ("enabled", 16),
                ("missing", 705),
                ("two", 23),
                ("total", 701),
            ]
        );
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Float64(1.0),
                Value::String("alpha".to_string()),
                Value::Bool(true),
                Value::Null,
                Value::Int64(2),
                Value::Float64(3.0),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_table_free_parameter_through_union_all() {
        // Arrange
        use_local_storage();
        let path = data_dir("table_free_parameters");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT $1::INT AS value UNION ALL SELECT 2 AS value",
                vec![Value::String("7".to_string())],
            )
            .expect("execute table-free set operation");

        // Assert
        assert_eq!(result.columns[0].type_oid, 23);
        assert_eq!(
            result.rows,
            vec![vec![Value::Float64(2.0)], vec![Value::Int64(7)]]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/recursive_cte_semantics.rs.
mod recursive_cte_semantics {
    use std::path::PathBuf;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use cassie::sql::ast::{CteQuery, QueryStatement, SetOperator};
    use cassie::sql::parser::parse_statement;
    use cassie::types::{DataType, FieldSchema, Schema, Value};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn seeded_integer_collection(name: &str, values: &[i64]) -> (Cassie, PathBuf) {
        use_local_storage();
        let path = data_dir(name);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let collection = format!("{name}_seed");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "n".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .expect("create seed collection");
        cassie.register_collection(
            &collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        for (index, value) in values.iter().enumerate() {
            cassie
                .midge
                .put_document(
                    &collection,
                    Some(format!("d{index}")),
                    serde_json::json!({"n": value}),
                )
                .expect("insert seed row");
        }
        (cassie, path.into())
    }

    fn integer_values(result: cassie::executor::QueryResult) -> Vec<i64> {
        result
            .rows
            .into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                Some(Value::Float64(value)) => value
                    .to_string()
                    .parse::<i64>()
                    .expect("expected an integral float"),
                other => panic!("expected integer value, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn should_preserve_recursive_union_operator_in_ast() {
        // Arrange
        let sql = "WITH RECURSIVE seq(n) AS (SELECT 1 UNION SELECT n + 1 FROM seq WHERE n < 2) SELECT n FROM seq";

        // Act
        let parsed = parse_statement(sql).expect("parse recursive union");

        // Assert
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("expected SELECT");
        };
        let CteQuery::Recursive { operator, .. } = &select.ctes[0].query else {
            panic!("expected recursive CTE");
        };
        assert_eq!(*operator, SetOperator::Union);
    }

    #[test]
    fn should_preserve_recursive_union_all_duplicates() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_union_all", &[1, 1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_union_all_seed UNION ALL SELECT n + 1 AS n FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
            vec![],
        )
        .expect("execute recursive UNION ALL");

        // Assert
        assert_eq!(integer_values(result), vec![1, 1, 2, 2]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_deduplicate_recursive_union_rows() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_union", &[1, 1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_union_seed UNION SELECT n + 1 AS n FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
            vec![],
        )
        .expect("execute recursive UNION");

        // Assert
        assert_eq!(integer_values(result), vec![1, 2]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_apply_recursive_cte_column_aliases() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_alias", &[1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(value) AS (SELECT n FROM recursive_alias_seed UNION ALL SELECT value + 1 FROM seq WHERE value < 2) SELECT value FROM seq ORDER BY value",
            vec![],
        )
        .expect("execute recursive CTE with aliases");

        // Assert
        assert_eq!(result.columns[0].name, "value");
        assert_eq!(integer_values(result), vec![1, 2]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_pass_parameters_through_recursive_terms() {
        // Arrange
        use_local_storage();
        let path = data_dir("recursive_parameters");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT $1::INT UNION ALL SELECT n + 1 FROM seq WHERE n < $2::INT) SELECT n FROM seq ORDER BY n",
            vec![Value::String("1".to_string()), Value::String("3".to_string())],
        )
        .expect("execute parameterized recursive CTE");

        // Assert
        assert_eq!(integer_values(result), vec![1, 2, 3]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_explain_recursive_cte_identity() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_explain", &[1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_explain_seed UNION ALL SELECT n + 1 FROM seq WHERE n < 2) SELECT n FROM seq",
                vec![],
            )
            .expect("explain recursive CTE");

        // Assert
        let Some(Value::String(plan)) = result.rows.first().and_then(|row| row.first()) else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("recursive_cte=seq"), "plan={plan}");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_recursive_anchor_self_reference() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_anchor_self", &[1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM seq UNION ALL SELECT n + 1 FROM seq WHERE n < 2) SELECT n FROM seq",
        vec![],
    );

        // Assert
        let error = result.expect_err("anchor self-reference must be rejected");
        assert!(error.to_string().contains("anchor"), "error={error}");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_recursive_projection_shape_mismatch() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_shape", &[1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_shape_seed UNION ALL SELECT n, n FROM seq WHERE n < 2) SELECT n FROM seq",
        vec![],
    );

        // Assert
        let error = result.expect_err("recursive arity mismatch must be rejected");
        assert!(error.to_string().contains("column count"), "error={error}");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_recursive_projection_type_mismatch() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_type", &[1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_type_seed UNION ALL SELECT 'wrong' FROM seq WHERE n < 2) SELECT n FROM seq",
        vec![],
    );

        // Assert
        let error = result.expect_err("recursive type mismatch must be rejected");
        assert!(error.to_string().contains("type"), "error={error}");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_multiple_recursive_references() {
        // Arrange
        let (cassie, path) = seeded_integer_collection("recursive_multiple", &[1]);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_multiple_seed UNION ALL SELECT seq.n FROM seq JOIN seq ON seq.n = seq.n WHERE seq.n < 2) SELECT n FROM seq",
        vec![],
    );

        // Assert
        let error = result.expect_err("multiple recursive references must be rejected");
        assert!(error.to_string().contains("multiple"), "error={error}");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_recursive_working_table_over_memory_budget() {
        // Arrange
        use_local_storage();
        let path = data_dir("recursive_temp_budget");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("create Cassie");
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 3) SELECT n FROM seq",
        vec![],
    );

        // Assert
        let error = result.expect_err("recursive working table should honor memory budget");
        assert!(
            error.to_string().contains("query memory budget exceeded"),
            "error={error}"
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/relational_semantic_correctness.rs.
mod relational_semantic_correctness {
    use cassie::app::{Cassie, CassieSession};
    use cassie::config::{CassieRuntimeConfig, OperatorSwitchingEnabled};
    use cassie::types::{Value, Vector};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn with_cassie(name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
        use_local_storage();
        let path = data_dir(name);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
        let _ = std::fs::remove_dir_all(path);
    }

    fn with_bounded_join_cassie(name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
        use_local_storage();
        let path = data_dir(name);
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.adaptive_execution_enabled = false;
        config.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_exact_large_bigint_ordering_across_relational_paths() {
        // Arrange
        with_cassie("semantic_large_bigints", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_large_bigints (value BIGINT)",
                    vec![],
                )
                .expect("create table");
            for value in [9_007_199_254_740_993_i64, 9_007_199_254_740_992_i64] {
                cassie
                    .execute_sql(
                        session,
                        "INSERT INTO semantic_large_bigints (value) VALUES ($1)",
                        vec![Value::Int64(value)],
                    )
                    .expect("insert value");
            }

            // Act
            let result = cassie
            .execute_sql(
                session,
                "SELECT value, row_number() OVER (ORDER BY value) AS ordinal FROM semantic_large_bigints ORDER BY value",
                vec![],
            )
            .expect("query");

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::Int64(9_007_199_254_740_992), Value::Int64(1)],
                    vec![Value::Int64(9_007_199_254_740_993), Value::Int64(2)],
                ]
            );
        });
    }

    #[test]
    fn should_compute_min_max_with_negative_mixed_numeric_inputs() {
        // Arrange
        with_cassie("semantic_numeric_minmax", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_negative_numbers (value BIGINT)",
                    vec![],
                )
                .expect("create negative table");
            for value in [-10_i64, -2, -30] {
                cassie
                    .execute_sql(
                        session,
                        &format!("INSERT INTO semantic_negative_numbers (value) VALUES ({value})"),
                        vec![],
                    )
                    .expect("insert negative value");
            }
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_integer_number (value BIGINT)",
                    vec![],
                )
                .expect("create integer table");
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_float_number (value FLOAT)",
                    vec![],
                )
                .expect("create float table");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO semantic_integer_number (value) VALUES (10)",
                    vec![],
                )
                .expect("insert integer");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO semantic_float_number (value) VALUES (-2.5)",
                    vec![],
                )
                .expect("insert float");

            // Act
            let negative = cassie
                .execute_sql(
                    session,
                    "SELECT min(value), max(value) FROM semantic_negative_numbers",
                    vec![],
                )
                .expect("negative aggregate");
            let mixed = cassie
            .execute_sql(
                session,
                "SELECT min(value), max(value) FROM (SELECT value FROM semantic_integer_number UNION ALL SELECT value FROM semantic_float_number) AS mixed_values",
                vec![],
            )
            .expect("mixed aggregate");

            // Assert
            assert_eq!(
                negative.rows,
                vec![vec![Value::Int64(-30), Value::Int64(-2)]]
            );
            assert_eq!(
                mixed.rows,
                vec![vec![Value::Float64(-2.5), Value::Int64(10)]]
            );
        });
    }

    #[test]
    fn should_propagate_order_by_expression_errors() {
        // Arrange
        with_cassie("semantic_sort_errors", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_sort_errors (label TEXT, divisor INT)",
                    vec![],
                )
                .expect("create table");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO semantic_sort_errors (label, divisor) VALUES ('invalid', 0)",
                    vec![],
                )
                .expect("insert row");

            // Act
            let error = cassie
                .execute_sql(
                    session,
                    "SELECT label FROM semantic_sort_errors ORDER BY 1 / divisor",
                    vec![],
                )
                .expect_err("division by zero must remain an ORDER BY error");

            // Assert
            assert!(error.to_string().contains("division by zero"));
        });
    }

    #[test]
    fn should_apply_postgres_null_ordering_across_relational_sorts() {
        // Arrange
        with_cassie("semantic_null_ordering", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_null_ordering (label TEXT, value INT)",
                    vec![],
                )
                .expect("create table");
            for sql in [
                "INSERT INTO semantic_null_ordering (label, value) VALUES ('low', 1)",
                "INSERT INTO semantic_null_ordering (label, value) VALUES ('high', 2)",
                "INSERT INTO semantic_null_ordering (label, value) VALUES ('missing', NULL)",
            ] {
                cassie
                    .execute_sql(session, sql, vec![])
                    .expect("insert row");
            }

            // Act
            let ascending = cassie
                .execute_sql(
                    session,
                    "SELECT label FROM semantic_null_ordering ORDER BY value ASC LIMIT 3",
                    vec![],
                )
                .expect("ascending query");
            let descending = cassie
                .execute_sql(
                    session,
                    "SELECT label FROM semantic_null_ordering ORDER BY value DESC LIMIT 3",
                    vec![],
                )
                .expect("descending query");
            let nulls_first = cassie
            .execute_sql(
                session,
                "SELECT label, row_number() OVER (ORDER BY value DESC NULLS FIRST) AS ordinal FROM semantic_null_ordering ORDER BY label",
                vec![],
            )
            .expect("window nulls first query");
            let nulls_last = cassie
            .execute_sql(
                session,
                "SELECT label, row_number() OVER (ORDER BY value DESC NULLS LAST) AS ordinal FROM semantic_null_ordering ORDER BY label",
                vec![],
            )
            .expect("window nulls last query");

            // Assert
            assert_eq!(
                ascending.rows,
                vec![
                    vec![Value::String("low".into())],
                    vec![Value::String("high".into())],
                    vec![Value::String("missing".into())],
                ]
            );
            assert_eq!(
                descending.rows,
                vec![
                    vec![Value::String("missing".into())],
                    vec![Value::String("high".into())],
                    vec![Value::String("low".into())],
                ]
            );
            assert_eq!(
                nulls_first.rows,
                vec![
                    vec![Value::String("high".into()), Value::Int64(2)],
                    vec![Value::String("low".into()), Value::Int64(3)],
                    vec![Value::String("missing".into()), Value::Int64(1)],
                ]
            );
            assert_eq!(
                nulls_last.rows,
                vec![
                    vec![Value::String("high".into()), Value::Int64(1)],
                    vec![Value::String("low".into()), Value::Int64(2)],
                    vec![Value::String("missing".into()), Value::Int64(3)],
                ]
            );
        });
    }

    #[test]
    fn should_preserve_positional_set_semantics_with_left_names() {
        // Arrange
        with_cassie("semantic_set_aliases", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_set_left (value TEXT)",
                    vec![],
                )
                .expect("create left");
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_set_right (value TEXT)",
                    vec![],
                )
                .expect("create right");
            for table in ["semantic_set_left", "semantic_set_right"] {
                cassie
                    .execute_sql(
                        session,
                        &format!("INSERT INTO {table} (value) VALUES ('shared')"),
                        vec![],
                    )
                    .expect("insert set row");
            }

            // Act
            let result = cassie
            .execute_sql(
                session,
                "SELECT value AS left_name FROM semantic_set_left INTERSECT SELECT value AS right_name FROM semantic_set_right",
                vec![],
            )
            .expect("set query");

            // Assert
            assert_eq!(result.columns[0].name, "left_name");
            assert_eq!(result.rows, vec![vec![Value::String("shared".into())]]);
        });
    }

    #[test]
    fn should_use_left_set_names_when_the_left_operand_is_empty() {
        // Arrange
        with_cassie("semantic_empty_left_set", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_empty_set_left (value TEXT)",
                    vec![],
                )
                .expect("create left table");
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_empty_set_right (value TEXT)",
                    vec![],
                )
                .expect("create right table");
            for value in ["a", "z"] {
                cassie
                    .execute_sql(
                        session,
                        &format!("INSERT INTO semantic_empty_set_right (value) VALUES ('{value}')"),
                        vec![],
                    )
                    .expect("insert right row");
            }

            // Act
            let result = cassie
            .execute_sql(
                session,
                "SELECT value AS left_name FROM semantic_empty_set_left UNION ALL SELECT value AS right_name FROM semantic_empty_set_right ORDER BY left_name DESC",
                vec![],
            )
            .expect("set query");

            // Assert
            assert_eq!(result.columns[0].name, "left_name");
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("z".into())],
                    vec![Value::String("a".into())],
                ]
            );
        });
    }

    #[test]
    fn should_keep_composite_relational_keys_collision_free() {
        // Arrange
        with_cassie("semantic_composite_keys", |cassie, session| {
            cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_composite_keys (bucket TEXT, first_part TEXT, second_part TEXT)",
                vec![],
            )
            .expect("create table");
            for sql in [
            "INSERT INTO semantic_composite_keys (bucket, first_part, second_part) VALUES ('one', 'a|4:b', 'c')",
            "INSERT INTO semantic_composite_keys (bucket, first_part, second_part) VALUES ('one', 'a', 'b|4:c')",
        ] {
            cassie.execute_sql(session, sql, vec![]).expect("insert row");
        }

            // Act
            let grouped = cassie
            .execute_sql(
                session,
                "SELECT first_part, second_part, count(*) AS total FROM semantic_composite_keys GROUP BY first_part, second_part ORDER BY first_part, second_part",
                vec![],
            )
            .expect("group query");
            let distinct = cassie
            .execute_sql(
                session,
                "SELECT DISTINCT first_part, second_part FROM semantic_composite_keys ORDER BY first_part, second_part",
                vec![],
            )
            .expect("distinct query");
            let distinct_on = cassie
            .execute_sql(
                session,
                "SELECT DISTINCT ON (first_part, second_part) first_part, second_part FROM semantic_composite_keys ORDER BY first_part, second_part",
                vec![],
            )
            .expect("distinct on query");
            let windowed = cassie
            .execute_sql(
                session,
                "SELECT first_part, second_part, row_number() OVER (PARTITION BY first_part, second_part ORDER BY first_part) AS row_number, rank() OVER (PARTITION BY bucket ORDER BY first_part, second_part) AS rank FROM semantic_composite_keys ORDER BY first_part, second_part",
                vec![],
            )
            .expect("window query");

            // Assert
            assert_eq!(grouped.rows.len(), 2);
            assert!(grouped.rows.iter().all(|row| row[2] == Value::Int64(1)));
            assert_eq!(distinct.rows.len(), 2);
            assert_eq!(distinct_on.rows.len(), 2);
            assert_eq!(windowed.rows.len(), 2);
            assert!(windowed.rows.iter().all(|row| row[2] == Value::Int64(1)));
            assert_eq!(
                windowed
                    .rows
                    .iter()
                    .map(|row| row[3].clone())
                    .collect::<Vec<_>>(),
                vec![Value::Int64(1), Value::Int64(2)]
            );
        });
    }

    #[test]
    fn should_match_integer_float_joins_with_scalar_numeric_equality() {
        // Arrange
        with_cassie("semantic_numeric_joins", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_join_ints (join_key BIGINT, label TEXT)",
                    vec![],
                )
                .expect("create ints");
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_join_floats (join_key FLOAT, label TEXT)",
                    vec![],
                )
                .expect("create floats");
            for sql in [
            "INSERT INTO semantic_join_ints (join_key, label) VALUES (1, 'small')",
            "INSERT INTO semantic_join_floats (join_key, label) VALUES (1.0, 'small')",
            "INSERT INTO semantic_join_floats (join_key, label) VALUES (9007199254740992.0, 'rounded')",
        ] {
            cassie.execute_sql(session, sql, vec![]).expect("insert row");
        }
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO semantic_join_ints (join_key, label) VALUES ($1, 'large')",
                    vec![Value::Int64(9_007_199_254_740_993)],
                )
                .expect("insert exact large integer");

            // Act
            let keyed = cassie
            .execute_sql(
                session,
                "SELECT semantic_join_ints.label FROM semantic_join_ints JOIN semantic_join_floats ON semantic_join_ints.join_key = semantic_join_floats.join_key ORDER BY semantic_join_ints.label",
                vec![],
            )
            .expect("keyed join");
            let scalar = cassie
            .execute_sql(
                session,
                "SELECT semantic_join_ints.label FROM semantic_join_ints JOIN semantic_join_floats ON true WHERE semantic_join_ints.join_key = semantic_join_floats.join_key ORDER BY semantic_join_ints.label",
                vec![],
            )
            .expect("scalar join");

            // Assert
            let expected = vec![vec![Value::String("small".into())]];
            assert_eq!(keyed.rows, expected);
            assert_eq!(scalar.rows, expected);
        });
    }

    #[test]
    fn should_preserve_numeric_equality_when_a_bounded_join_side_is_indexed() {
        // Arrange
        with_bounded_join_cassie("semantic_indexed_numeric_join", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_indexed_join_ints (join_key BIGINT, label TEXT)",
                    vec![],
                )
                .expect("create integer table");
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE semantic_indexed_join_floats (join_key FLOAT, label TEXT)",
                    vec![],
                )
                .expect("create float table");
            cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_indexed_join_ints (join_key, label) VALUES (1, 'matched')",
                vec![],
            )
            .expect("insert integer row");
            cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_indexed_join_floats (join_key, label) VALUES (1.0, 'matched')",
                vec![],
            )
            .expect("insert float row");
            cassie
            .execute_sql(
                session,
                "CREATE INDEX semantic_indexed_join_floats_key_idx ON semantic_indexed_join_floats USING btree (join_key)",
                vec![],
            )
            .expect("create float index");

            // Act
            let result = cassie
            .execute_sql(
                session,
                "SELECT semantic_indexed_join_ints.label FROM semantic_indexed_join_ints JOIN semantic_indexed_join_floats ON semantic_indexed_join_ints.join_key = semantic_indexed_join_floats.join_key LIMIT 10",
                vec![],
            )
            .expect("bounded indexed join");

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("matched".into())]]);
        });
    }

    #[test]
    fn should_evaluate_same_dimension_vector_parameters_independently() {
        // Arrange
        with_cassie("semantic_vector_parameters", |cassie, session| {
            let sql = "SELECT vector_distance($1, '[1,0]')";

            // Act
            let first = cassie
                .execute_sql(
                    session,
                    sql,
                    vec![Value::Vector(Vector::new(vec![1.0, 0.0]))],
                )
                .expect("first vector query");
            let second = cassie
                .execute_sql(
                    session,
                    sql,
                    vec![Value::Vector(Vector::new(vec![0.0, 1.0]))],
                )
                .expect("second vector query");

            // Assert
            assert_eq!(first.rows, vec![vec![Value::Float64(0.0)]]);
            let Value::Float64(distance) = second.rows[0][0] else {
                panic!("expected floating vector distance");
            };
            assert!((distance - 2.0_f64.sqrt()).abs() < 1e-12);
        });
    }

    #[test]
    fn should_invalidate_immutable_udf_results_on_recreation() {
        // Arrange
        with_cassie("semantic_udf_recreate", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    r#"CREATE FUNCTION semantic_recreated(x INT) RETURNS INT IMMUTABLE AS "x""#,
                    vec![],
                )
                .expect("create original function");
            let original = cassie
                .execute_sql(session, "SELECT semantic_recreated(1)", vec![])
                .expect("execute original");
            cassie
                .execute_sql(session, "DROP FUNCTION semantic_recreated", vec![])
                .expect("drop function");
            cassie
                .execute_sql(
                    session,
                    r#"CREATE FUNCTION semantic_recreated(x INT) RETURNS INT IMMUTABLE AS "x + 1""#,
                    vec![],
                )
                .expect("recreate function");

            // Act
            let recreated = cassie
                .execute_sql(session, "SELECT semantic_recreated(1)", vec![])
                .expect("execute recreated");

            // Assert
            assert_eq!(original.rows, vec![vec![Value::Int64(1)]]);
            assert_eq!(recreated.rows, vec![vec![Value::Int64(2)]]);
        });
    }
}

// Formerly tests/scalar_functions.rs.
mod scalar_functions {
    use std::env;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use cassie::sql::ast::{Expr, QueryStatement, SelectItem};
    use cassie::sql::parser::parse_statement;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use tokio_postgres::{NoTls, SimpleQueryMessage};

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    struct CompatibilityServer {
        data_dir: String,
        addr: SocketAddr,
        server: tokio::task::JoinHandle<()>,
    }

    impl CompatibilityServer {
        async fn start(label: &str) -> Self {
            use_local_storage();
            let data_dir = data_dir(label);
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone()).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(async move {
                let _ =
                    cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
                        .await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;

            Self {
                data_dir,
                addr,
                server,
            }
        }

        async fn connect(&self) -> (tokio_postgres::Client, tokio::task::JoinHandle<()>) {
            let mut config = tokio_postgres::Config::new();
            config.host("127.0.0.1");
            config.port(self.addr.port());
            config.user("root");
            config.password("postgres");
            config.dbname("postgres");

            let (client, connection) = config.connect(NoTls).await.expect("connect tokio-postgres");
            let connection = tokio::spawn(async move {
                connection
                    .await
                    .expect("tokio-postgres connection task should stay healthy");
            });

            (client, connection)
        }

        async fn shutdown(self, connection: tokio::task::JoinHandle<()>) {
            connection.abort();
            self.server.abort();
            let _ = connection.await;
            let _ = self.server.await;
            let _ = std::fs::remove_dir_all(self.data_dir);
        }
    }

    #[test]
    fn should_parse_common_scalar_function_calls() {
        // Arrange
        let sql =
        "SELECT concat(lower(title), coalesce(status, 'unknown'), substring(code, 2, 3)) FROM docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let SelectItem::Function { function, .. } = &statement.projection[0] else {
            panic!("expected function projection");
        };
        assert_eq!(function.name, "concat");
        assert_eq!(function.args.len(), 3);
        assert!(matches!(&function.args[0], Expr::Function(inner) if inner.name == "lower"));
        assert!(matches!(&function.args[1], Expr::Function(inner) if inner.name == "coalesce"));
        assert!(matches!(&function.args[2], Expr::Function(inner) if inner.name == "substring"));
    }

    #[test]
    fn should_execute_string_scalar_functions_in_query_path() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let path = data_dir("string_helpers");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_string_helpers";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(table, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"id": "d1", "title": "  Alpha  "}),
            )

            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT lower(title) AS lowered, upper(title) AS uppered, trim(title) AS trimmed, substring(trim(title), 2, 3) AS sliced, concat(lower(trim(title)), '-', 'suffix') AS combined, length(trim(title)) AS length_value, len(trim(title)) AS len_value FROM scalar_string_helpers WHERE lower(trim(title)) = 'alpha' ORDER BY len(trim(title)) DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns.len(), 7);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("  alpha  ".to_string()));
        assert_eq!(result.rows[0][1], Value::String("  ALPHA  ".to_string()));
        assert_eq!(result.rows[0][2], Value::String("Alpha".to_string()));
        assert_eq!(result.rows[0][3], Value::String("lph".to_string()));
        assert_eq!(
            result.rows[0][4],
            Value::String("alpha-suffix".to_string())
        );
        assert_eq!(result.rows[0][5], Value::Int64(5));
        assert_eq!(result.rows[0][6], Value::Int64(5));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_null_numeric_scalar_functions() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let path = data_dir("coalesce_abs");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_null_helpers";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(table, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"id": "d1", "title": null, "score": -4}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                table,
                Some("d2".to_string()),
                serde_json::json!({"id": "d2", "title": "beta", "score": 9}),
            )

            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, coalesce(title, 'fallback') AS resolved, abs(score) AS absolute_score FROM scalar_null_helpers ORDER BY abs(score) DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(
            result.rows[0],
            vec![
                Value::String("d2".to_string()),
                Value::String("beta".to_string()),
                Value::Int64(9),
            ]
        );
        assert_eq!(
            result.rows[1],
            vec![
                Value::String("d1".to_string()),
                Value::String("fallback".to_string()),
                Value::Int64(4),
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_short_circuit_coalesce_before_evaluating_later_arguments() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("coalesce_short_circuit");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            let table = "scalar_coalesce_short_circuit";

            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(table, schema.clone())
                .unwrap();
            cassie.register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    table,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "alpha", "score": 7}),
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT coalesce(title, lower(score)) FROM scalar_coalesce_short_circuit",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_scalar_function_with_invalid_arity() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("arity_error");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            let table = "scalar_arity_error";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(table, schema.clone())
                .unwrap();
            cassie.register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    table,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            // Act
            let result = cassie.execute_sql(
                &session,
                "SELECT lower(title, title) FROM scalar_arity_error",
                vec![],
            );

            // Assert
            let error = result.expect_err("query should fail");
            assert!(error.to_string().contains("lower"));
            assert!(error.to_string().contains("expects"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_scalar_function_with_unsupported_type() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("type_error");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            let table = "scalar_type_error";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(table, schema.clone())
                .unwrap();
            cassie.register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    table,
                    Some("d1".to_string()),
                    serde_json::json!({"score": 7}),
                )
                .unwrap();

            // Act
            let result = cassie.execute_sql(
                &session,
                "SELECT lower(score) FROM scalar_type_error",
                vec![],
            );

            // Assert
            let error = result.expect_err("query should fail");
            assert!(error.to_string().to_lowercase().contains("text"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_scalar_functions_through_pgwire() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let server = CompatibilityServer::start("pgwire").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let messages = tokio::time::timeout(
            Duration::from_secs(5),
            client.simple_query(
                "SELECT lower('ALPHA'), upper('beta'), trim('  gamma  '), substring('delta', 2, 3), concat('a', 'b'), coalesce(NULL, 'fallback'), length('zoo'), len('zoo'), abs(-4)",
            ),
        )
        .await
        .expect("query should complete within the timeout")
        .expect("pgwire query result");

        // Assert
        let row = messages
            .into_iter()
            .find_map(|message| match message {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            })
            .expect("query should return a row");
        assert_eq!(row.get(0), Some("alpha"));
        assert_eq!(row.get(1), Some("BETA"));
        assert_eq!(row.get(2), Some("gamma"));
        assert_eq!(row.get(3), Some("elt"));
        assert_eq!(row.get(4), Some("ab"));
        assert_eq!(row.get(5), Some("fallback"));
        assert_eq!(row.get(6), Some("3"));
        assert_eq!(row.get(7), Some("3"));
        assert_eq!(row.get(8), Some("4"));

        drop(client);
        server.shutdown(connection).await;
    });
    }

    #[test]
    fn should_execute_user_defined_functions_after_builtin_expansion() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("udf_regression");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            let table = "scalar_udf_regression";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(table, schema.clone())
                .unwrap();
            cassie.register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    table,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "Alpha"}),
                )
                .unwrap();

            cassie
                .execute_sql(
                    &session,
                    r#"CREATE FUNCTION echo_text(x TEXT) RETURNS TEXT AS "x""#,
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT echo_text(lower(title)) FROM scalar_udf_regression",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_bucket_timestamps_into_fixed_windows() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let path = data_dir("time_bucket_fixed_windows");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE time_bucket_fixed_windows (event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO time_bucket_fixed_windows (event_at) VALUES ('2024-01-01T00:00:00Z')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO time_bucket_fixed_windows (event_at) VALUES ('2024-01-01T00:14:59Z')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO time_bucket_fixed_windows (event_at) VALUES ('2024-01-01T00:15:00Z')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('15 minutes', event_at) AS bucket FROM time_bucket_fixed_windows ORDER BY event_at",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("2024-01-01T00:00:00Z".to_string())],
                vec![Value::String("2024-01-01T00:00:00Z".to_string())],
                vec![Value::String("2024-01-01T00:15:00Z".to_string())],
            ]
        );
        assert_eq!(result.columns[0].data_type, "timestamp");

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_bucket_timestamps_with_custom_origin() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let path = data_dir("time_bucket_origin");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_time_bucket_origin (id INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_time_bucket_origin (id) VALUES (1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 hour', '1969-12-31T23:30:00Z'), time_bucket('15 minutes', '2024-01-01T00:19:00Z', '2024-01-01T00:05:00Z') FROM scalar_time_bucket_origin",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            result.rows[0],
            vec![
                Value::String("1969-12-31T23:00:00Z".to_string()),
                Value::String("2024-01-01T00:05:00Z".to_string()),
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_validate_time_bucket_nulls_errors() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let path = data_dir("time_bucket_errors");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_time_bucket_errors (event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_time_bucket_errors (event_at) VALUES (NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let null_result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('15 minutes', event_at) FROM scalar_time_bucket_errors",
                vec![],
            )
            .unwrap();
        let zero_width = cassie.execute_sql(
            &session,
            "SELECT time_bucket('0 seconds', '2024-01-01T00:00:00Z') FROM scalar_time_bucket_errors",
            vec![],
        );
        let month_width = cassie.execute_sql(
            &session,
            "SELECT time_bucket('1 month', '2024-01-01T00:00:00Z') FROM scalar_time_bucket_errors",
            vec![],
        );
        let bad_type = cassie.execute_sql(
            &session,
            "SELECT time_bucket(15, '2024-01-01T00:00:00Z') FROM scalar_time_bucket_errors",
            vec![],
        );

        // Assert
        assert_eq!(null_result.rows, vec![vec![Value::Null]]);
        assert!(zero_width.unwrap_err().to_string().contains("positive"));
        assert!(month_width.unwrap_err().to_string().contains("calendar"));
        assert!(bad_type.unwrap_err().to_string().contains("duration string"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_time_bucket_grouping_having_ordering() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let path = data_dir("time_bucket_grouping");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_time_bucket_grouping (event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        for event_at in [
            "2024-01-01T00:01:00Z",
            "2024-01-01T00:14:00Z",
            "2024-01-01T00:16:00Z",
        ] {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO scalar_time_bucket_grouping (event_at) VALUES ('{event_at}')"
                    ),
                    vec![],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('15 minutes', event_at) AS bucket, count(*) FROM scalar_time_bucket_grouping GROUP BY time_bucket('15 minutes', event_at) HAVING count(*) > 1 ORDER BY bucket",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::String("2024-01-01T00:00:00Z".to_string()),
                Value::Int64(2),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_time_bucket_through_pgwire() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start("time_bucket_pgwire").await;
            let (client, connection) = server.connect().await;

            // Act
            client
                .simple_query("CREATE TABLE scalar_time_bucket_pgwire (event_at TIMESTAMP)")
                .await
                .unwrap();
            client
            .simple_query(
                "INSERT INTO scalar_time_bucket_pgwire (event_at) VALUES ('2024-01-01T00:29:00Z')",
            )
            .await
            .unwrap();
            let messages = client
                .simple_query(
                    "SELECT time_bucket('15 minutes', event_at) FROM scalar_time_bucket_pgwire",
                )
                .await
                .unwrap();

            // Assert
            let row = messages
                .iter()
                .find_map(|message| match message {
                    SimpleQueryMessage::Row(row) => Some(row),
                    _ => None,
                })
                .expect("time_bucket row");
            assert_eq!(row.get(0), Some("2024-01-01T00:15:00Z"));

            drop(client);
            server.shutdown(connection).await;
        });
    }
}

// Formerly tests/sql_semantic_regressions.rs.
mod sql_semantic_regressions {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn cassie_for(label: &str) -> (Cassie, cassie::app::CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    #[test]
    fn should_evaluate_subtraction_chains_left_to_right() {
        // Arrange
        let (cassie, session, path) = cassie_for("left_associative_subtraction");

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT 10 - 3 - 2 AS value", vec![])
            .expect("execute subtraction chain");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_evaluate_division_chains_left_to_right() {
        // Arrange
        let (cassie, session, path) = cassie_for("left_associative_division");

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT 100.0 / 10 / 2 AS value", vec![])
            .expect("execute division chain");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_mixed_same_precedence_evaluation_order() {
        // Arrange
        let (cassie, session, path) = cassie_for("left_associative_mixed");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT 10 - 3 + 2 AS additive, 100 / 10 * 2 AS multiplicative",
                vec![],
            )
            .expect("execute mixed arithmetic chains");

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Float64(9.0), Value::Float64(20.0)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_bigint_scalar_arithmetic_precision() {
        // Arrange
        let (cassie, session, path) = cassie_for("bigint_scalar_arithmetic");

        // Act
        let exact = cassie
            .execute_sql(
                &session,
                "SELECT 9007199254740993 + 9007199254740993 AS added, 9007199254740995 - 9007199254740993 AS subtracted, CAST(3002399751580331 AS BIGINT) * CAST(3 AS BIGINT) AS multiplied, 1 + 1.5 AS mixed",
                vec![],
            )
            .expect("execute exact bigint arithmetic");
        // Assert
        assert_eq!(
            exact.rows,
            vec![vec![
                Value::Int64(18_014_398_509_481_986),
                Value::Int64(2),
                Value::Int64(9_007_199_254_740_993),
                Value::Float64(2.5),
            ]]
        );
        assert_eq!(
            exact
                .columns
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![20, 20, 20, 701]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_integer_column_arithmetic_exact_when_combined_with_a_bare_literal() {
        // Arrange
        let (cassie, session, path) = cassie_for("integer_literal_arithmetic");
        cassie
            .execute_sql(&session, "CREATE TABLE t (n INT)", vec![])
            .expect("create table");
        cassie
            .execute_sql(&session, "INSERT INTO t (n) VALUES (1)", vec![])
            .expect("insert row");

        // Act
        let column_plus_literal = cassie
            .execute_sql(&session, "SELECT n + 1 FROM t", vec![])
            .expect("execute column plus literal");
        let literal_plus_column = cassie
            .execute_sql(&session, "SELECT 1 + n FROM t", vec![])
            .expect("execute literal plus column");
        let column_plus_column = cassie
            .execute_sql(&session, "SELECT n + n FROM t", vec![])
            .expect("execute column plus column");
        let pure_literal = cassie
            .execute_sql(&session, "SELECT 1 + 2 AS total", vec![])
            .expect("execute pure literal arithmetic");

        // Assert
        assert_eq!(column_plus_literal.rows, vec![vec![Value::Int64(2)]]);
        assert_eq!(column_plus_literal.columns[0].type_oid, 20);
        assert_eq!(literal_plus_column.rows, vec![vec![Value::Int64(2)]]);
        assert_eq!(literal_plus_column.columns[0].type_oid, 20);
        assert_eq!(column_plus_column.rows, vec![vec![Value::Int64(2)]]);
        // A literal-only expression with no integer operand keeps its
        // existing float-by-default contract (see
        // should_execute_table_free_literal_alias_cast_projection).
        assert_eq!(pure_literal.rows, vec![vec![Value::Float64(3.0)]]);
        assert_eq!(pure_literal.columns[0].type_oid, 701);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_bigint_scalar_arithmetic_overflow() {
        // Arrange
        let (cassie, session, path) = cassie_for("bigint_scalar_overflow");

        // Act
        let overflow_errors = [
            "SELECT 9223372036854775807 + 9007199254740993",
            "SELECT -9223372036854775808 - 9007199254740993",
            "SELECT 9007199254740993 * 9007199254740993",
        ]
        .map(|sql| {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect_err("reject bigint overflow")
        });

        // Assert
        for error in overflow_errors {
            assert!(error.to_string().contains("integer overflow"));
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_round_trip_bigint_literals_exactly() {
        // Arrange
        let (cassie, session, path) = cassie_for("bigint_literal_round_trip");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE bigint_literal_probe (wide BIGINT)",
                vec![],
            )
            .expect("create bigint probe");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO bigint_literal_probe VALUES (9007199254740993), (9223372036854775807)",
                vec![],
            )
            .expect("insert exact wide bigints");
        let above_f64_boundary = cassie
            .execute_sql(
                &session,
                "SELECT wide FROM bigint_literal_probe WHERE wide = 9007199254740993",
                vec![],
            )
            .expect("select bigint above exact float boundary");
        let maximum = cassie
            .execute_sql(
                &session,
                "SELECT wide FROM bigint_literal_probe WHERE wide = 9223372036854775807",
                vec![],
            )
            .expect("select maximum bigint");

        // Assert
        assert_eq!(
            above_f64_boundary.rows,
            vec![vec![Value::Int64(9_007_199_254_740_993)]]
        );
        assert_eq!(maximum.rows, vec![vec![Value::Int64(i64::MAX)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_round_trip_minimum_bigint_literal_exactly() {
        // Arrange
        let (cassie, session, path) = cassie_for("minimum_bigint_literal");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE minimum_bigint_probe (wide BIGINT)",
                vec![],
            )
            .expect("create bigint probe");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO minimum_bigint_probe VALUES (-9223372036854775808)",
                vec![],
            )
            .expect("insert minimum bigint");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT wide FROM minimum_bigint_probe WHERE wide = -9223372036854775808",
                vec![],
            )
            .expect("select minimum bigint");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(i64::MIN)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_non_finite_float_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("non_finite_float_literal");

        // Act
        let literal_error = cassie
            .execute_sql(&session, "SELECT 1e400", vec![])
            .expect_err("reject non-finite float literal");
        let cast_error = cassie
            .execute_sql(&session, "SELECT CAST('1e400' AS FLOAT)", vec![])
            .expect_err("reject non-finite float cast");

        // Assert
        assert!(literal_error
            .to_string()
            .contains("numeric literal out of range"));
        assert!(cast_error
            .to_string()
            .contains("cannot cast value to FLOAT"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_out_of_range_integer_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("out_of_range_integer_literal");

        // Act
        let error = cassie
            .execute_sql(&session, "SELECT 9223372036854775808", vec![])
            .expect_err("reject integer outside bigint range");

        // Assert
        assert!(error.to_string().contains("numeric literal out of range"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_ungrouped_projection_column() {
        // Arrange
        let (cassie, session, path) = cassie_for("ungrouped_projection");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE grouped_projection_probe (grp TEXT, name TEXT, amount INT)",
                vec![],
            )
            .expect("create grouped projection probe");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT grp, name, SUM(amount) FROM grouped_projection_probe GROUP BY grp",
                vec![],
            )
            .expect_err("reject ungrouped projection");

        // Assert
        assert!(error.to_string().contains("must appear in the GROUP BY"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_ungrouped_column_inside_expression() {
        // Arrange
        let (cassie, session, path) = cassie_for("ungrouped_expression");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE grouped_expression_probe (grp TEXT, name TEXT, amount INT)",
                vec![],
            )
            .expect("create grouped expression probe");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT grp, upper(name), SUM(amount) FROM grouped_expression_probe GROUP BY grp",
                vec![],
            )
            .expect_err("reject ungrouped expression");

        // Assert
        assert!(error.to_string().contains("must appear in the GROUP BY"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_grouped_projection_columns() {
        // Arrange
        let (cassie, session, path) = cassie_for("valid_grouped_projection");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE valid_grouped_probe (grp TEXT, amount INT)",
                vec![],
            )
            .expect("create grouped projection probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO valid_grouped_probe VALUES ('a', 2), ('a', 3)",
                vec![],
            )
            .expect("seed grouped projection probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT grp, SUM(amount) FROM valid_grouped_probe GROUP BY grp",
                vec![],
            )
            .expect("execute valid grouped projection");

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("a".to_string()), Value::Int64(5)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_ambiguous_unqualified_join_column() {
        // Arrange
        let (cassie, session, path) = cassie_for("ambiguous_join_column");
        cassie
            .execute_sql(&session, "CREATE TABLE amb_left (label TEXT)", vec![])
            .expect("create left relation");
        cassie
            .execute_sql(&session, "CREATE TABLE amb_right (label TEXT)", vec![])
            .expect("create right relation");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT label FROM amb_left JOIN amb_right ON true",
                vec![],
            )
            .expect_err("reject ambiguous column");

        // Assert
        assert!(error
            .to_string()
            .contains("column reference 'label' is ambiguous"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_qualified_join_column() {
        // Arrange
        let (cassie, session, path) = cassie_for("qualified_join_column");
        cassie
            .execute_sql(&session, "CREATE TABLE qual_left (label TEXT)", vec![])
            .expect("create left relation");
        cassie
            .execute_sql(&session, "CREATE TABLE qual_right (label TEXT)", vec![])
            .expect("create right relation");
        cassie
            .execute_sql(&session, "INSERT INTO qual_left VALUES ('left')", vec![])
            .expect("seed left relation");
        cassie
            .execute_sql(&session, "INSERT INTO qual_right VALUES ('right')", vec![])
            .expect("seed right relation");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT qual_left.label FROM qual_left JOIN qual_right ON true",
                vec![],
            )
            .expect("execute qualified projection");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("left".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_compare_uuid_column_to_valid_text_literal() {
        // Arrange
        let (cassie, session, path) = cassie_for("uuid_text_comparison");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE uuid_probe (item_id TEXT, item_uuid UUID)",
                vec![],
            )
            .expect("create uuid probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO uuid_probe VALUES ('row-1', '550e8400-e29b-41d4-a716-446655440000')",
                vec![],
            )
            .expect("seed uuid probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM uuid_probe WHERE item_uuid = '550e8400-e29b-41d4-a716-446655440000'",
                vec![],
            )
            .expect("compare uuid to text literal");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("row-1".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_uuid_comparison_literal() {
        // Arrange
        let (cassie, session, path) = cassie_for("malformed_uuid_comparison");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE malformed_uuid_probe (item_uuid UUID)",
                vec![],
            )
            .expect("create uuid probe");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT item_uuid FROM malformed_uuid_probe WHERE item_uuid = 'not-a-uuid'",
                vec![],
            )
            .expect_err("reject malformed uuid literal");

        // Assert
        assert!(error.to_string().contains("invalid UUID literal"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_compare_bytea_column_to_valid_text_literal() {
        // Arrange
        let (cassie, session, path) = cassie_for("bytea_text_comparison");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE bytea_probe (item_id TEXT, payload BYTEA)",
                vec![],
            )
            .expect("create bytea probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO bytea_probe VALUES ('row-1', '\\x01020a')",
                vec![],
            )
            .expect("seed bytea probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM bytea_probe WHERE payload = '\\x01020a'",
                vec![],
            )
            .expect("compare bytea to text literal");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("row-1".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_bytea_comparison_literal() {
        // Arrange
        let (cassie, session, path) = cassie_for("malformed_bytea_comparison");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE malformed_bytea_probe (payload BYTEA)",
                vec![],
            )
            .expect("create bytea probe");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM malformed_bytea_probe WHERE payload = '\\xzz'",
                vec![],
            )
            .expect_err("reject malformed bytea literal");

        // Assert
        assert!(error.to_string().contains("invalid BYTEA literal"));
        let _ = std::fs::remove_dir_all(path);
    }
}
// Formerly tests/window_frames.rs.
mod window_frames {
    use cassie::app::{Cassie, CassieError};
    use cassie::sql::ast::{
        QueryStatement, SelectItem, WindowFrameBound, WindowFrameExclusion, WindowFrameUnit,
    };
    use cassie::sql::parse_statement;
    use cassie::types::Value;
    use tokio_postgres::{Config, NoTls};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    use super::support_pgwire as wire;

    fn execute_window_query(
        label: &str,
        query: &str,
    ) -> Result<cassie::executor::QueryResult, CassieError> {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE window_frame_values (category TEXT, ordinal INT, value INT)",
                vec![],
            )
            .expect("create window fixture");
        for (category, ordinal, value) in [
            ("a", 1, 10),
            ("a", 2, 20),
            ("a", 3, 20),
            ("a", 4, 30),
            ("b", 1, 99),
        ] {
            cassie
            .execute_sql(
                &session,
                &format!(
                    "INSERT INTO window_frame_values (category, ordinal, value) VALUES ('{category}', {ordinal}, {value})"
                ),
                vec![],
            )
            .expect("insert window fixture row");
        }
        let result = cassie.execute_sql(&session, query, vec![]);
        let _ = std::fs::remove_dir_all(path);
        result
    }

    fn values_for_column(result: &cassie::executor::QueryResult, index: usize) -> Vec<Value> {
        result.rows.iter().map(|row| row[index].clone()).collect()
    }

    #[test]
    fn should_parse_explicit_rows_frame_bounds() {
        // Arrange
        let sql = "SELECT first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) FROM window_frame_values";

        // Act
        let parsed = parse_statement(sql).expect("parse explicit ROWS frame");

        // Assert
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("expected SELECT");
        };
        let SelectItem::WindowFunction { function, .. } = &select.projection[0] else {
            panic!("expected window function");
        };
        assert_eq!(
            function.frame,
            Some(cassie::sql::ast::WindowFrame {
                unit: WindowFrameUnit::Rows,
                start: WindowFrameBound::Preceding(1),
                end: WindowFrameBound::CurrentRow,
                exclusion: WindowFrameExclusion::NoOthers,
            })
        );
    }

    #[test]
    fn should_apply_ordered_default_rows_frame() {
        // Arrange
        let query = "SELECT ordinal, first_value(value) OVER (PARTITION BY category ORDER BY ordinal) AS first_value, last_value(value) OVER (PARTITION BY category ORDER BY ordinal) AS last_value FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result =
            execute_window_query("window_default_rows", query).expect("execute default frame");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(1), Value::Int64(10), Value::Int64(10)],
                vec![Value::Int64(2), Value::Int64(10), Value::Int64(20)],
                vec![Value::Int64(3), Value::Int64(10), Value::Int64(20)],
                vec![Value::Int64(4), Value::Int64(10), Value::Int64(30)],
            ]
        );
    }

    #[test]
    fn should_apply_whole_partition_rows_frame() {
        // Arrange
        let query = "SELECT ordinal, first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING), last_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result =
            execute_window_query("window_whole_partition", query).expect("execute whole frame");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(1), Value::Int64(10), Value::Int64(30)],
                vec![Value::Int64(2), Value::Int64(10), Value::Int64(30)],
                vec![Value::Int64(3), Value::Int64(10), Value::Int64(30)],
                vec![Value::Int64(4), Value::Int64(10), Value::Int64(30)],
            ]
        );
    }

    #[test]
    fn should_apply_bounded_preceding_following_rows_frame() {
        // Arrange
        let query = "SELECT ordinal, first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING), last_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result =
            execute_window_query("window_bounded_rows", query).expect("execute bounded frame");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)],
                vec![Value::Int64(2), Value::Int64(10), Value::Int64(20)],
                vec![Value::Int64(3), Value::Int64(20), Value::Int64(30)],
                vec![Value::Int64(4), Value::Int64(20), Value::Int64(30)],
            ]
        );
    }

    #[test]
    fn should_keep_frame_independent_functions() {
        // Arrange
        let query = "SELECT ordinal, rank() OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND CURRENT ROW), lag(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND CURRENT ROW), lead(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND CURRENT ROW) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result = execute_window_query("window_frame_independent", query)
            .expect("execute frame-independent functions");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::Null,
                    Value::Int64(20)
                ],
                vec![
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::Int64(10),
                    Value::Int64(20)
                ],
                vec![
                    Value::Int64(3),
                    Value::Int64(3),
                    Value::Int64(20),
                    Value::Int64(30)
                ],
                vec![
                    Value::Int64(4),
                    Value::Int64(4),
                    Value::Int64(20),
                    Value::Null
                ],
            ]
        );
    }

    #[test]
    fn should_apply_rows_frame_without_collapsing_peers() {
        // Arrange
        let query = "SELECT ordinal, last_value(value) OVER (PARTITION BY category ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result = execute_window_query("window_rows_peers", query).expect("execute peer frame");

        // Assert
        assert_eq!(
            values_for_column(&result, 1),
            vec![
                Value::Int64(10),
                Value::Int64(20),
                Value::Int64(20),
                Value::Int64(30),
            ]
        );
    }

    #[test]
    fn should_handle_empty_window_partition() {
        // Arrange
        let empty_query = "SELECT first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM window_frame_values WHERE category = 'missing'";
        let single_query = "SELECT first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING), last_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM window_frame_values WHERE category = 'b'";

        // Act
        let empty =
            execute_window_query("window_empty_partition", empty_query).expect("empty partition");
        let single = execute_window_query("window_single_partition", single_query)
            .expect("single partition");

        // Assert
        assert!(empty.rows.is_empty());
        assert_eq!(single.rows, vec![vec![Value::Int64(99), Value::Int64(99)]]);
    }

    #[test]
    fn should_apply_range_window_frame_to_peers() {
        // Arrange
        let query = "SELECT ordinal, first_value(value) OVER (ORDER BY value RANGE BETWEEN CURRENT ROW AND CURRENT ROW), last_value(value) OVER (ORDER BY value RANGE BETWEEN CURRENT ROW AND CURRENT ROW) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result =
            execute_window_query("window_range_peers", query).expect("execute RANGE frame");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(1), Value::Int64(10), Value::Int64(10)],
                vec![Value::Int64(2), Value::Int64(20), Value::Int64(20)],
                vec![Value::Int64(3), Value::Int64(20), Value::Int64(20)],
                vec![Value::Int64(4), Value::Int64(30), Value::Int64(30)],
            ]
        );
    }

    #[test]
    fn should_keep_large_integer_range_offsets_exact() {
        // Arrange
        use_local_storage();
        let path = data_dir("window_range_exact_integer");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exact_range_values (ordinal BIGINT)",
                vec![],
            )
            .expect("create table");
        for value in [9_007_199_254_740_992_i64, 9_007_199_254_740_993_i64] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO exact_range_values (ordinal) VALUES ($1)",
                    vec![Value::Int64(value)],
                )
                .expect("insert exact integer");
        }

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT ordinal, last_value(ordinal) OVER (ORDER BY ordinal RANGE BETWEEN 0 PRECEDING AND 0 FOLLOWING) FROM exact_range_values ORDER BY ordinal",
            vec![],
        )
        .expect("execute exact RANGE frame");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::Int64(9_007_199_254_740_992),
                    Value::Int64(9_007_199_254_740_992),
                ],
                vec![
                    Value::Int64(9_007_199_254_740_993),
                    Value::Int64(9_007_199_254_740_993),
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_apply_groups_window_frame_offsets() {
        // Arrange
        let query = "SELECT ordinal, first_value(value) OVER (ORDER BY value GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW), last_value(value) OVER (ORDER BY value GROUPS BETWEEN CURRENT ROW AND 1 FOLLOWING) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result =
            execute_window_query("window_groups_offsets", query).expect("execute GROUPS frame");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)],
                vec![Value::Int64(2), Value::Int64(10), Value::Int64(30)],
                vec![Value::Int64(3), Value::Int64(10), Value::Int64(30)],
                vec![Value::Int64(4), Value::Int64(20), Value::Int64(30)],
            ]
        );
    }

    #[test]
    fn should_apply_window_frame_exclusions() {
        // Arrange
        let query = "SELECT ordinal, last_value(value) OVER (ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE CURRENT ROW) AS without_current, first_value(value) OVER (ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE GROUP) AS without_group, first_value(value) OVER (ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE TIES) AS without_ties FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

        // Act
        let result = execute_window_query("window_exclusions", query).expect("execute exclusions");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::Int64(1),
                    Value::Int64(30),
                    Value::Int64(20),
                    Value::Int64(10)
                ],
                vec![
                    Value::Int64(2),
                    Value::Int64(30),
                    Value::Int64(10),
                    Value::Int64(10)
                ],
                vec![
                    Value::Int64(3),
                    Value::Int64(30),
                    Value::Int64(10),
                    Value::Int64(10)
                ],
                vec![
                    Value::Int64(4),
                    Value::Int64(20),
                    Value::Int64(10),
                    Value::Int64(10)
                ],
            ]
        );
    }

    #[test]
    fn should_reject_invalid_window_frame_order() {
        // Arrange
        let query = "SELECT first_value(value) OVER (ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND 1 PRECEDING) FROM window_frame_values";

        // Act
        let error = execute_window_query("window_invalid_order", query)
            .expect_err("invalid frame order should be rejected");

        // Assert
        assert!(
            matches!(error, CassieError::Unsupported(message) if message.contains("frame bounds"))
        );
    }

    #[test]
    fn should_reject_negative_window_frame_offset() {
        // Arrange
        let query = "SELECT first_value(value) OVER (ORDER BY ordinal ROWS BETWEEN -1 PRECEDING AND CURRENT ROW) FROM window_frame_values";

        // Act
        let error = execute_window_query("window_negative_offset", query)
            .expect_err("negative frame offset should be rejected");

        // Assert
        assert!(matches!(error, CassieError::Unsupported(message) if message.contains("negative")));
    }

    #[test]
    fn should_default_deserialized_window_frame_exclusion() {
        // Arrange
        let serialized = r#"{"unit":"Rows","start":"UnboundedPreceding","end":"CurrentRow"}"#;

        // Act
        let frame: cassie::sql::ast::WindowFrame =
            serde_json::from_str(serialized).expect("deserialize legacy frame");

        // Assert
        assert_eq!(frame.exclusion, WindowFrameExclusion::NoOthers);
    }

    #[test]
    fn should_not_return_unsupported_window_frame_sqlstate() {
        // Arrange
        use_local_storage();
        let path = data_dir("window_pgwire_error");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        let mut config = Config::new();
        config.host("127.0.0.1");
        config.port(server.addr.port());
        config.user("root");
        config.dbname("postgres");
        config.password("postgres");
        let (client, connection) = config.connect(NoTls).await.expect("connect pgwire");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });

        // Act
        let error = client
            .query(
                "SELECT first_value(value) OVER (ORDER BY ordinal RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM missing_window_frame_values",
                &[],
            )
            .await
            .expect_err("missing relation should be reported");

        // Assert
        assert_ne!(
            error
                .as_db_error()
                .expect("database error")
                .code()
                .code(),
            "0A000"
        );

        drop(client);
        connection.abort();
        let _ = connection.await;
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/sql_typed_literal_predicates.rs.
mod sql_typed_literal_predicates {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn cassie_for(label: &str) -> (Cassie, cassie::app::CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    #[test]
    fn should_match_uuid_in_typed_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("uuid_in_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE uuid_in_probe (item_id TEXT, item_uuid UUID)",
                vec![],
            )
            .expect("create UUID probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO uuid_in_probe VALUES ('row-1', '550e8400-e29b-41d4-a716-446655440000'), ('row-2', '550e8400-e29b-41d4-a716-446655440001')",
                vec![],
            )
            .expect("seed UUID probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM uuid_in_probe WHERE item_uuid IN ('550e8400-e29b-41d4-a716-446655440001', '550e8400-e29b-41d4-a716-446655440002')",
                vec![],
            )
            .expect("execute UUID IN predicate");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("row-2".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_uuid_in_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("malformed_uuid_in_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE malformed_uuid_in_probe (item_uuid UUID)",
                vec![],
            )
            .expect("create UUID probe");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT item_uuid FROM malformed_uuid_in_probe WHERE item_uuid IN ('not-a-uuid')",
                vec![],
            )
            .expect_err("reject malformed UUID IN literal");

        // Assert
        assert!(error.to_string().contains("invalid UUID literal"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_match_uuid_between_typed_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("uuid_between_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE uuid_between_probe (item_id TEXT, item_uuid UUID)",
                vec![],
            )
            .expect("create UUID probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO uuid_between_probe VALUES ('before', '450e8400-e29b-41d4-a716-446655440000'), ('inside', '550e8400-e29b-41d4-a716-446655440001'), ('after', '650e8400-e29b-41d4-a716-446655440000')",
                vec![],
            )
            .expect("seed UUID probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM uuid_between_probe WHERE item_uuid BETWEEN '550e8400-e29b-41d4-a716-446655440000' AND '550e8400-e29b-41d4-a716-446655440002'",
                vec![],
            )
            .expect("execute UUID BETWEEN predicate");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("inside".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_uuid_between_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("malformed_uuid_between_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE malformed_uuid_between_probe (item_uuid UUID)",
                vec![],
            )
            .expect("create UUID probe");

        // Act
        let errors = [
            cassie
                .execute_sql(
                    &session,
                    "SELECT item_uuid FROM malformed_uuid_between_probe WHERE item_uuid BETWEEN 'not-a-uuid' AND '550e8400-e29b-41d4-a716-446655440002'",
                    vec![],
                )
                .expect_err("reject malformed UUID lower bound"),
            cassie
                .execute_sql(
                    &session,
                    "SELECT item_uuid FROM malformed_uuid_between_probe WHERE item_uuid BETWEEN '550e8400-e29b-41d4-a716-446655440000' AND 'not-a-uuid'",
                    vec![],
                )
                .expect_err("reject malformed UUID upper bound"),
        ];

        // Assert
        assert!(errors
            .iter()
            .all(|error| error.to_string().contains("invalid UUID literal")));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_match_bytea_in_typed_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("bytea_in_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE bytea_in_probe (item_id TEXT, payload BYTEA)",
                vec![],
            )
            .expect("create BYTEA probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO bytea_in_probe VALUES ('row-1', '\\x0102'), ('row-2', '\\x0304')",
                vec![],
            )
            .expect("seed BYTEA probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM bytea_in_probe WHERE payload IN ('\\x0304', '\\x0506')",
                vec![],
            )
            .expect("execute BYTEA IN predicate");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("row-2".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_bytea_in_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("malformed_bytea_in_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE malformed_bytea_in_probe (payload BYTEA)",
                vec![],
            )
            .expect("create BYTEA probe");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM malformed_bytea_in_probe WHERE payload IN ('\\xzz')",
                vec![],
            )
            .expect_err("reject malformed BYTEA IN literal");

        // Assert
        assert!(error.to_string().contains("invalid BYTEA literal"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_match_bytea_between_typed_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("bytea_between_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE bytea_between_probe (item_id TEXT, payload BYTEA)",
                vec![],
            )
            .expect("create BYTEA probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO bytea_between_probe VALUES ('inside', '\\x0180'), ('after', '\\x0200')",
                vec![],
            )
            .expect("seed BYTEA probe");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM bytea_between_probe WHERE payload BETWEEN '\\x0100' AND '\\x01ff'",
                vec![],
            )
            .expect("execute BYTEA BETWEEN predicate");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("inside".to_string())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_bytea_between_literals() {
        // Arrange
        let (cassie, session, path) = cassie_for("malformed_bytea_between_literals");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE malformed_bytea_between_probe (payload BYTEA)",
                vec![],
            )
            .expect("create BYTEA probe");

        // Act
        let errors = [
            cassie
                .execute_sql(
                    &session,
                    "SELECT payload FROM malformed_bytea_between_probe WHERE payload BETWEEN '\\x0' AND '\\x01ff'",
                    vec![],
                )
                .expect_err("reject malformed BYTEA lower bound"),
            cassie
                .execute_sql(
                    &session,
                    "SELECT payload FROM malformed_bytea_between_probe WHERE payload BETWEEN '\\x0100' AND 'not-bytea'",
                    vec![],
                )
                .expect_err("reject malformed BYTEA upper bound"),
        ];

        // Assert
        assert!(errors
            .iter()
            .all(|error| error.to_string().contains("invalid BYTEA literal")));
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/typed_literal_canonicalization.rs.
mod typed_literal_canonicalization {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    #[test]
    fn should_canonicalize_typed_predicate_literals() {
        // Arrange
        use_local_storage();
        let path = data_dir("canonical_typed_literals");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE canonical_typed_probe (item_id TEXT, item_uuid UUID, payload BYTEA)",
                vec![],
            )
            .expect("create typed probe");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO canonical_typed_probe VALUES ('MixedCase', '550e8400-e29b-41d4-a716-446655440000', '\\x0a0b')",
                vec![],
            )
            .expect("seed typed probe");

        // Act
        let predicates = [
            "item_uuid = '550E8400E29B41D4A716446655440000'",
            "'550E8400E29B41D4A716446655440000' = item_uuid",
            "item_uuid IN ('550E8400E29B41D4A716446655440000')",
            "item_uuid BETWEEN '550E8400E29B41D4A716446655440000' AND '550E8400E29B41D4A716446655440000'",
            "payload = '\\x0A0B'",
            "'\\x0A0B' = payload",
            "payload IN ('\\x0A0B')",
            "payload BETWEEN '\\x0A0B' AND '\\x0A0B'",
        ];
        let results = predicates.map(|predicate| {
            cassie
                .execute_sql(
                    &session,
                    &format!("SELECT item_id FROM canonical_typed_probe WHERE {predicate}"),
                    vec![],
                )
                .expect("execute canonical typed predicate")
        });
        let text_control = cassie
            .execute_sql(
                &session,
                "SELECT item_id FROM canonical_typed_probe WHERE item_id = 'mixedcase'",
                vec![],
            )
            .expect("execute text control");

        // Assert
        for result in results {
            assert_eq!(
                result.rows,
                vec![vec![Value::String("MixedCase".to_string())]]
            );
        }
        assert!(text_control.rows.is_empty());
        let _ = std::fs::remove_dir_all(path);
    }
}

// Promotion evidence for the "Types and casts" row in docs/feature-support.md,
// tracked in docs/query-promotion-evidence.md. A boundary-value matrix rather
// than a seeded differential fixture: CAST has no alternate access path to
// compare against, so evidence here is exact-value and exact-error coverage
// at every documented type's representable range and rejection boundary.
mod type_cast_promotion_evidence {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    #[test]
    fn should_cast_values_at_every_documented_type_boundary() {
        // Arrange
        use_local_storage();
        let path = data_dir("type_cast_boundary_matrix_ok");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        let ok_cases: &[(&str, Value)] = &[
            ("CAST('32767' AS SMALLINT)", Value::Int64(32767)),
            ("CAST('-32768' AS SMALLINT)", Value::Int64(-32768)),
            ("CAST('2147483647' AS INT)", Value::Int64(2_147_483_647)),
            ("CAST('-2147483648' AS INT)", Value::Int64(-2_147_483_648)),
            (
                "CAST('9223372036854775807' AS BIGINT)",
                Value::Int64(9_223_372_036_854_775_807),
            ),
            (
                "CAST('-9223372036854775808' AS BIGINT)",
                Value::Int64(-9_223_372_036_854_775_808),
            ),
            ("CAST('3.5' AS FLOAT)", Value::Float64(3.5)),
            ("CAST('true' AS BOOLEAN)", Value::Bool(true)),
            ("CAST('F' AS BOOLEAN)", Value::Bool(false)),
            ("CAST('abc' AS CHAR(3))", Value::String("abc".to_string())),
            (
                "CAST('abc' AS VARCHAR(3))",
                Value::String("abc".to_string()),
            ),
            (
                "CAST('550e8400-e29b-41d4-a716-446655440000' AS UUID)",
                Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
            ),
            (
                "CAST('\\x0a0b' AS BYTEA)",
                Value::String("\\x0a0b".to_string()),
            ),
            (
                "CAST('2024-01-01T09:00:00+02:00' AS TIMESTAMP)",
                Value::String("2024-01-01T07:00:00Z".to_string()),
            ),
            ("CAST(NULL AS INT)", Value::Null),
        ];

        // Act
        let selected = ok_cases
            .iter()
            .map(|(expr, _)| {
                cassie
                    .execute_sql(&session, &format!("SELECT {expr}"), vec![])
                    .unwrap_or_else(|error| panic!("{expr} should cast: {error}"))
                    .rows
            })
            .collect::<Vec<_>>();

        // Assert
        for ((expr, expected), rows) in ok_cases.iter().zip(selected) {
            assert_eq!(rows, vec![vec![expected.clone()]], "{expr}");
        }

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_casts_beyond_every_documented_type_boundary() {
        // Arrange
        use_local_storage();
        let path = data_dir("type_cast_boundary_matrix_rejected");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        let rejected_cases: &[&str] = &[
            "CAST('32768' AS SMALLINT)",
            "CAST('-32769' AS SMALLINT)",
            "CAST('2147483648' AS INT)",
            "CAST('9223372036854775808' AS BIGINT)",
            "CAST('inf' AS FLOAT)",
            "CAST('nan' AS FLOAT)",
            "CAST('not-a-number' AS INT)",
            "CAST('maybe' AS BOOLEAN)",
            "CAST('abcd' AS CHAR(3))",
            "CAST('abcd' AS VARCHAR(3))",
            "CAST('not-a-uuid' AS UUID)",
            "CAST('not-hex' AS BYTEA)",
            "CAST('not-a-timestamp' AS TIMESTAMP)",
            "CAST('not-a-date' AS DATE)",
            "CAST('25:00:00' AS TIME)",
        ];

        // Act
        let results = rejected_cases
            .iter()
            .map(|expr| cassie.execute_sql(&session, &format!("SELECT {expr}"), vec![]))
            .collect::<Vec<_>>();

        // Assert
        for (expr, result) in rejected_cases.iter().zip(results) {
            assert!(result.is_err(), "{expr} should be rejected");
        }

        let _ = std::fs::remove_dir_all(path);
    }
}

// Regressions for issue #276. Rows carry the internal document identity only
// under the reserved `_id` key, and a bare `id` is rewritten to it at the
// logical-plan level (see `planner::logical::reserved_id`). That rewrite has
// to reach every source shape: a plan built anywhere but the planner's own
// entry points once missed it, and `id` then resolved to nothing at all —
// silently NULL, including into a table on `INSERT ... SELECT`.
mod reserved_id_source_resolution {
    use super::support_sql as support;

    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn identity_of(cassie: &Cassie, session: &CassieSession, sql: &str) -> String {
        let result = cassie.execute_sql(session, sql, vec![]).expect("select id");
        match &result.rows[0][0] {
            Value::String(id) => id.clone(),
            other => panic!("expected an internal identity string, got {other:?}"),
        }
    }

    #[test]
    fn should_resolve_id_to_the_internal_identity_in_every_source_shape() {
        // Arrange
        use_local_storage();
        let path = data_dir("reserved_id_undeclared_sources");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE reserved_id_left (name TEXT)",
            "CREATE TABLE reserved_id_right (name TEXT)",
            "CREATE TABLE reserved_id_sink (doc TEXT)",
            "INSERT INTO reserved_id_left (name) VALUES ('alice')",
            "INSERT INTO reserved_id_right (name) VALUES ('alice')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect(statement);
        }
        let identity = identity_of(&cassie, &session, "SELECT id FROM reserved_id_left");

        // Act
        let joined = cassie
            .execute_sql(
                &session,
                "SELECT id FROM reserved_id_left JOIN reserved_id_right ON reserved_id_left.name = reserved_id_right.name",
                vec![],
            )
            .expect("join");
        let derived = cassie
            .execute_sql(
                &session,
                "SELECT id FROM (SELECT id, name FROM reserved_id_left) s",
                vec![],
            )
            .expect("derived table");
        let cte = cassie
            .execute_sql(
                &session,
                "WITH c AS (SELECT * FROM reserved_id_left) SELECT id FROM c",
                vec![],
            )
            .expect("cte reference");
        let union = cassie
            .execute_sql(
                &session,
                "SELECT id FROM reserved_id_left UNION SELECT id FROM reserved_id_left",
                vec![],
            )
            .expect("set operation");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO reserved_id_sink (doc) SELECT id FROM reserved_id_left",
                vec![],
            )
            .expect("insert select");
        let inserted = cassie
            .execute_sql(&session, "SELECT doc FROM reserved_id_sink", vec![])
            .expect("read back insert select");

        // Assert
        let expected = vec![vec![Value::String(identity.clone())]];
        assert_eq!(joined.rows, expected, "join lost the internal identity");
        assert_eq!(
            derived.rows, expected,
            "derived table lost the internal identity"
        );
        assert_eq!(
            cte.rows, expected,
            "CTE reference lost the internal identity"
        );
        assert_eq!(
            union.rows, expected,
            "set operation lost the internal identity"
        );
        assert_eq!(
            inserted.rows, expected,
            "INSERT ... SELECT stored NULL instead of the internal identity"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_resolve_a_declared_id_column_in_every_source_shape() {
        // Arrange
        use_local_storage();
        let path = data_dir("reserved_id_declared_sources");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE declared_id_rows (id INT, name TEXT)",
            "CREATE TABLE declared_id_other (id INT, note TEXT)",
            "INSERT INTO declared_id_rows (id, name) VALUES (42, 'alice')",
            "INSERT INTO declared_id_rows (id, name) VALUES (7, 'carol')",
            "INSERT INTO declared_id_other (id, note) VALUES (99, 'linked')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect(statement);
        }

        // Act
        let derived = cassie
            .execute_sql(
                &session,
                "SELECT id FROM (SELECT id, name FROM declared_id_rows) s ORDER BY id",
                vec![],
            )
            .expect("derived table");
        let cte = cassie
            .execute_sql(
                &session,
                "WITH c AS (SELECT * FROM declared_id_rows) SELECT id FROM c ORDER BY id",
                vec![],
            )
            .expect("cte reference");
        let union = cassie
            .execute_sql(
                &session,
                "SELECT id FROM declared_id_rows UNION SELECT id FROM declared_id_other",
                vec![],
            )
            .expect("set operation");

        // Assert
        let ordered = vec![vec![Value::Int64(7)], vec![Value::Int64(42)]];
        assert_eq!(derived.rows, ordered);
        assert_eq!(cte.rows, ordered);
        let mut union_values = union
            .rows
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>();
        union_values.sort_by_key(|value| match value {
            Value::Int64(number) => *number,
            other => panic!("expected the declared id values, got {other:?}"),
        });
        assert_eq!(
            union_values,
            vec![Value::Int64(7), Value::Int64(42), Value::Int64(99)]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_return_a_value_for_a_column_aliased_to_the_internal_identity_name() {
        // Arrange
        use_local_storage();
        let path = data_dir("reserved_id_aliased_output");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE aliased_identity_rows (name TEXT)",
            "INSERT INTO aliased_identity_rows (name) VALUES ('alice')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect(statement);
        }

        // Act
        let aliased = cassie
            .execute_sql(
                &session,
                "SELECT name AS _id FROM aliased_identity_rows",
                vec![],
            )
            .expect("aliased output column");

        // Assert
        // A row narrower than its own column list desynchronizes the pgwire
        // RowDescription/DataRow pair, so the widths must agree.
        assert_eq!(aliased.columns.len(), 1);
        assert_eq!(aliased.rows, vec![vec![Value::String("alice".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_drop_a_declared_id_column_but_never_the_internal_identity() {
        // Arrange
        use_local_storage();
        let path = data_dir("reserved_id_drop_column");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE droppable_id_rows (id INT, name TEXT)",
            "INSERT INTO droppable_id_rows (id, name) VALUES (42, 'alice')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect(statement);
        }

        // Act
        let dropped = cassie.execute_sql(
            &session,
            "ALTER TABLE droppable_id_rows DROP COLUMN id",
            vec![],
        );
        let reserved = cassie.execute_sql(
            &session,
            "ALTER TABLE droppable_id_rows DROP COLUMN _id",
            vec![],
        );

        // Assert
        assert!(
            dropped.is_ok(),
            "a declared id column is an ordinary column and must be droppable: {dropped:?}"
        );
        let message = reserved
            .expect_err("dropping _id must be rejected")
            .to_string();
        assert!(
            message.contains("reserved field '_id'"),
            "unexpected error for dropping the internal identity: {message}"
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
