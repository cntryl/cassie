#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
use cassie::executor;
use cassie::planner::logical::LogicalPlan;
use cassie::planner::physical::PhysicalPlan;
use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
use cassie::sql::binder;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

#[path = "support/executor.rs"]
mod support;
use support::*;

#[test]
fn should_execute_simple_filtered_query() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("smoke");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "exec_smoke";

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
            .put_document(collection, None, serde_json::json!({"title": "alpha"}))
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_smoke WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.rows.len(), 1);
        match &result.rows[0][0] {
            Value::String(value) => assert_eq!(value, "alpha"),
            _ => panic!("expected string in first column"),
        }

        let _ = std::fs::remove_dir_all(path);
    });
}

#[tokio::test]
async fn execute_query_with_non_recursive_cte() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_cte_simple";

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

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "WITH docs_cte AS (SELECT title FROM exec_cte_simple) SELECT title FROM docs_cte ORDER BY title",
            vec![],
        )

.unwrap();

    assert_eq!(result.columns[0].name, "title");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
    assert_eq!(result.rows[1][0], Value::String("beta".to_string()));
}

#[tokio::test]
async fn execute_query_with_ordered_cte_dependencies() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_cte_dependency";

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
            Some("d1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"title": "beta"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "WITH first AS (SELECT title FROM exec_cte_dependency), second AS (SELECT title FROM first WHERE title = 'beta') SELECT title FROM second",
            vec![],
        )

.unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::String("beta".to_string()));
}

#[tokio::test]
async fn execute_query_passes_params_to_cte_and_main_query() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_cte_params";

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
            Some("d1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"title": "beta"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "WITH filtered_docs AS (SELECT title FROM exec_cte_params WHERE title = $1) SELECT title FROM filtered_docs WHERE title = $1",
            vec![Value::String("alpha".to_string())],
        )

.unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
}

#[tokio::test]
async fn execute_recursive_cte_until_stabilization() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_cte_recursive";

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
            serde_json::json!({"n": 1}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_recursive WHERE n = 1 UNION ALL SELECT n FROM seq WHERE n = 1) SELECT n FROM seq ORDER BY n",
            vec![],
        )

.unwrap();

    let values = result
        .rows
        .into_iter()
        .map(|row| match row.first() {
            Some(Value::Int64(value)) => *value,
            _ => panic!("expected integer value"),
        })
        .collect::<Vec<_>>();
    assert_eq!(values, vec![1]);
}

#[tokio::test]
async fn execute_recursive_cte_enforces_depth_limit_when_no_stabilization() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_cte_infinite";

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
            serde_json::json!({"n": 1}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_infinite WHERE n = 1 UNION ALL SELECT n + 1 AS n FROM seq) SELECT n FROM seq",
            vec![],
        )
        ;

    assert!(result.is_err());
}

#[tokio::test]
async fn execute_query_with_alias_and_filters() {
    with_fallback();
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-exec-{}", Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "exec_docs_alias";

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
            None,
            serde_json::json!({
                "title": "alpha",
                "body": "world news",
                "embedding": [1.0, 2.0],
            }),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            None,
            serde_json::json!({
                "title": "beta",
                "body": "world peace",
                "embedding": [0.5, 1.5],
            }),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            None,
            serde_json::json!({
                "title": "gamma",
                "body": "misc",
                "embedding": [2.0, 0.5],
            }),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT title AS title_out, search_score(body, 'world') AS score FROM exec_docs_alias WHERE body LIKE '%world%' OR title = 'gamma' ORDER BY score DESC, id ASC LIMIT 2",
            vec![],
        )

.expect("query should execute");

    assert_eq!(result.columns[0].name, "title_out");
    assert_eq!(result.columns[1].name, "score");
    assert_eq!(result.rows.len(), 2);
    for row in result.rows {
        assert_eq!(row.len(), 2);
        match &row[1] {
            Value::Float64(_) => {}
            _ => panic!("expected float score"),
        }
    }

    let params = vec![Value::String("alpha".to_string())];
    let parsed =
        parser::parse_statement("SELECT title FROM exec_docs_alias WHERE title = $1").unwrap();
    binder::bind(parsed, &cassie.catalog).unwrap();
    let param_result = cassie
        .execute_sql(
            &session,
            "SELECT title FROM exec_docs_alias WHERE title = $1",
            params,
        )
        .expect("parameterized query should run");

    assert_eq!(param_result.rows.len(), 1);
    match &param_result.rows[0][0] {
        Value::String(value) => assert_eq!(value, "alpha"),
        _ => panic!("expected string in first column"),
    }
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn execute_query_respects_boolean_precedence() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_precedence";

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
            serde_json::json!({"title": "alpha", "body": "x"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"title": "beta", "body": "x"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"title": "beta", "body": "y"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d4".to_string()),
            serde_json::json!({"title": "gamma", "body": "x"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_precedence WHERE title = 'alpha' OR title = 'beta' AND body = 'x' ORDER BY id",
            vec![],
        )

.expect("query should execute");

    assert_eq!(result.rows.len(), 2);

    let ids = result
        .rows
        .into_iter()
        .map(|row| match &row[0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected id value"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["d1".to_string(), "d2".to_string()]);
}

#[tokio::test]
async fn execute_query_parentheses_override_precedence() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_precedence_paren";

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
            serde_json::json!({"title": "alpha", "body": "x"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"title": "beta", "body": "x"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"title": "beta", "body": "y"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d4".to_string()),
            serde_json::json!({"title": "gamma", "body": "x"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_precedence_paren WHERE (title = 'alpha' OR title = 'beta') AND body = 'x' ORDER BY id",
            vec![],
        )

.expect("query should execute");

    assert_eq!(result.rows.len(), 2);

    let ids = result
        .rows
        .into_iter()
        .map(|row| match &row[0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected id value"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["d1".to_string(), "d2".to_string()]);
}

#[test]
fn should_execute_query_across_multiple_batches_without_truncation() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_multi_batch";

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

        for index in 0..1105 {
            let id = format!("d{index:04}");
            let title = format!("doc-{index:04}");
            cassie
                .midge
                .put_document(collection, Some(id), serde_json::json!({ "title": title }))
                .unwrap();
        }

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_multi_batch ORDER BY title ASC LIMIT 5 OFFSET 1095",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 5);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "d1095".to_string(),
                "d1096".to_string(),
                "d1097".to_string(),
                "d1098".to_string(),
                "d1099".to_string(),
            ]
        );
    });
}

#[test]
fn should_preserve_filtered_projection_across_multiple_batches() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_multi_batch_filter";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
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

        for index in 0..1105 {
            let id = format!("d{index:04}");
            let title = format!("doc-{index:04}");
            let status = if index % 2 == 0 { "keep" } else { "drop" };
            cassie
                .midge
                .put_document(
                    collection,
                    Some(id),
                    serde_json::json!({ "title": title, "status": status }),
                )
                .unwrap();
        }

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_multi_batch_filter WHERE status = 'keep' ORDER BY title ASC LIMIT 5 OFFSET 510",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 5);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "d1020".to_string(),
                "d1022".to_string(),
                "d1024".to_string(),
                "d1026".to_string(),
                "d1028".to_string(),
            ]
        );
    });
}

#[test]
fn should_sort_with_stable_tiebreaker() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_stable_tie";

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
                Some("z".to_string()),
                serde_json::json!({"title": "same", "body": "value"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("a".to_string()),
                serde_json::json!({"title": "same", "body": "value"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("m".to_string()),
                serde_json::json!({"title": "same", "body": "value"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_stable_tie ORDER BY 1 ASC",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 3);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["a".to_string(), "m".to_string(), "z".to_string()]);
    });
}

#[test]
fn should_sort_by_projection_alias_with_different_case() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_hybrid_alias_case";

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
            );

        cassie
            .midge
            .put_document(
                collection,
                Some("z".to_string()),
                serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("a".to_string()),
                serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("m".to_string()),
                serde_json::json!({"body": "red", "embedding": [0.0, 1.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS Score FROM exec_hybrid_alias_case ORDER BY SCORE DESC",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 3);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["a".to_string(), "m".to_string(), "z".to_string()]);
    });
}

#[test]
fn should_project_missing_columns_as_null() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_missing_projection_column";

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
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM exec_missing_projection_column",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[0][1], Value::Null);
    });
}

#[test]
fn should_sort_by_unprojected_column_before_projection() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_order_by_unprojected_field";

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
                Some("id1".to_string()),
                serde_json::json!({"title": "title-a", "body": "zzz"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("id2".to_string()),
                serde_json::json!({"title": "title-b", "body": "aaa"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_order_by_unprojected_field ORDER BY body ASC",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("title-b".to_string()));
        assert_eq!(result.rows[1][0], Value::String("title-a".to_string()));
    });
}

#[test]
fn should_be_deterministic_for_repeated_execution_metadata() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_repeated_metadata";

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
                Some("id1".to_string()),
                serde_json::json!({"title": "alpha", "body": "first"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("id2".to_string()),
                serde_json::json!({"title": "beta", "body": "second"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM exec_repeated_metadata ORDER BY title ASC",
                vec![],
            )
            .expect("query should execute");
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM exec_repeated_metadata ORDER BY title ASC",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(first.command, second.command);
        let first_columns = first
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.data_type.clone()))
            .collect::<Vec<_>>();
        let second_columns = second
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.data_type.clone()))
            .collect::<Vec<_>>();
        assert_eq!(first_columns, second_columns);
        assert_eq!(first.rows, second.rows);
    });
}

#[test]
fn should_project_complex_values_through_filtered_ordered_scan() {
    // Arrange
    with_fallback();
    let path = data_dir("zero_copy_projected_complex_values");
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
                "CREATE TABLE zero_copy_projected_complex_values (title TEXT, score INT, payload JSON, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "zero_copy_projected_complex_values",
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "score": 2,
                    "payload": {"nested": ["a", "b"]},
                    "embedding": [1.0, 2.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "zero_copy_projected_complex_values",
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "score": 1,
                    "embedding": [3.0, 4.0],
                }),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT payload, embedding FROM zero_copy_projected_complex_values WHERE title = 'alpha' ORDER BY score ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::Null);
        assert_eq!(
            result.rows[0][1],
            Value::Vector(cassie::types::Vector::new(vec![3.0, 4.0]))
        );
        assert_eq!(
            result.rows[1][0],
            Value::Json(serde_json::json!({"nested": ["a", "b"]}))
        );
        assert_eq!(
            result.rows[1][1],
            Value::Vector(cassie::types::Vector::new(vec![1.0, 2.0]))
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
