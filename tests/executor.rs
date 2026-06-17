use cassie::app::Cassie;
use cassie::sql::binder;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use std::env;
use uuid::Uuid;

fn with_fallback() {
    env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

#[test]
fn should_execute_simple_filtered_query() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;
        cassie
            .midge
            .put_document(collection, None, serde_json::json!({"title": "alpha"}))
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_smoke WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.rows.len(), 1);
        match &result.rows[0][0] {
            Value::String(value) => assert_eq!(value, "alpha"),
            _ => panic!("expected string in first column"),
        }
    });
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
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

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
        .await
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
        .await
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
        .await
        .unwrap();

    let session = cassie.create_session("tester", None).await;
    let result = cassie
        .execute_sql(
            &session,
            "SELECT title AS title_out, search_score(body, 'world') AS score FROM exec_docs_alias WHERE body LIKE '%world%' OR title = 'gamma' ORDER BY score DESC, id ASC LIMIT 2",
            vec![],
        )
        .await
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
    binder::bind(parsed, &cassie.catalog).await.unwrap();
    let param_result = cassie
        .execute_sql(
            &session,
            "SELECT title FROM exec_docs_alias WHERE title = $1",
            params,
        )
        .await
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
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"title": "alpha", "body": "x"}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"title": "beta", "body": "x"}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"title": "beta", "body": "y"}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d4".to_string()),
            serde_json::json!({"title": "gamma", "body": "x"}),
        )
        .await
        .unwrap();

    let session = cassie.create_session("tester", None).await;
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_precedence WHERE title = 'alpha' OR title = 'beta' AND body = 'x' ORDER BY id",
            vec![],
        )
        .await
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
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"title": "alpha", "body": "x"}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"title": "beta", "body": "x"}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"title": "beta", "body": "y"}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d4".to_string()),
            serde_json::json!({"title": "gamma", "body": "x"}),
        )
        .await
        .unwrap();

    let session = cassie.create_session("tester", None).await;
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_precedence_paren WHERE (title = 'alpha' OR title = 'beta') AND body = 'x' ORDER BY id",
            vec![],
        )
        .await
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
async fn execute_query_filters_by_vector_score_function() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_vector_score_filter";

    let schema = Schema {
        fields: vec![FieldSchema {
            name: "embedding".to_string(),
            data_type: DataType::Vector(2),
            nullable: true,
        }],
    };

    cassie
        .midge
        .create_collection(collection, schema.clone())
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"embedding": [1.0, 0.0]}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"embedding": [0.0, 1.0]}),
        )
        .await
        .unwrap();

    let session = cassie.create_session("tester", None).await;
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_score(embedding, '[1,0]') AS score FROM exec_vector_score_filter WHERE vector_score(embedding, '[1,0]') > 0.5",
            vec![],
        )
        .await
        .expect("query should execute");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.columns[0].name, "id");
    assert_eq!(result.columns[1].name, "score");
    assert_eq!(
        result.rows[0][0],
        cassie::types::Value::String("d1".to_string())
    );
}

#[tokio::test]
async fn execute_query_orders_by_vector_distance_function_parameterized() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_vector_order_func";

    let schema = Schema {
        fields: vec![FieldSchema {
            name: "embedding".to_string(),
            data_type: DataType::Vector(2),
            nullable: true,
        }],
    };

    cassie
        .midge
        .create_collection(collection, schema.clone())
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"embedding": [1.0, 0.0]}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"embedding": [0.2, 0.0]}),
        )
        .await
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"embedding": [10.0, 10.0]}),
        )
        .await
        .unwrap();

    let session = cassie.create_session("tester", None).await;
    let params = vec![cassie::types::Value::String("[1,0]".to_string())];
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_vector_order_func ORDER BY vector_distance(embedding, $1) ASC",
            params,
        )
        .await
        .expect("query should execute");

    let ids = result
        .rows
        .into_iter()
        .map(|row| match &row[0] {
            cassie::types::Value::String(id) => id.clone(),
            _ => panic!("expected string id"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec!["d2".to_string(), "d1".to_string(), "d3".to_string()]
    );
}

#[tokio::test]
async fn execute_query_supports_projection_aliases_for_function_columns() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
    let collection = "exec_function_alias";

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
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"title": "alpha", "body": "lorem world"}),
        )
        .await
        .unwrap();

    let session = cassie.create_session("tester", None).await;
    let result = cassie
        .execute_sql(
            &session,
            "SELECT search_score(body, 'world') AS score FROM exec_function_alias WHERE title = 'alpha'",
            vec![],
        )
        .await
        .expect("query should execute");

    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.columns[0].name, "score");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].len(), 1);
    match &result.rows[0][0] {
        cassie::types::Value::Float64(score) => assert!(*score > 0.0),
        _ => panic!("expected float score"),
    }
}

#[test]
fn should_skip_offset_then_take_limit() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_offset_limit";

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "a"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "b"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "c"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d4".to_string()),
                serde_json::json!({"title": "d"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d5".to_string()),
                serde_json::json!({"title": "e"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_offset_limit ORDER BY title ASC LIMIT 2 OFFSET 2",
                vec![],
            )
            .await
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 2);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["d3".to_string(), "d4".to_string()]);
    });
}

#[test]
fn should_default_missing_offset_to_zero_in_execution() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_default_offset";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "c"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "a"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "b"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let default_offset_result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1",
                vec![],
            )
            .await
            .expect("query should execute");

        let explicit_offset_result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1 OFFSET 0",
                vec![],
            )
            .await
            .expect("query should execute");

        // Assert
        assert_eq!(default_offset_result.rows.len(), 1);
        assert_eq!(explicit_offset_result.rows.len(), 1);
        assert_eq!(default_offset_result.rows, explicit_offset_result.rows);
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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("z".to_string()),
                serde_json::json!({"title": "same", "body": "value"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("a".to_string()),
                serde_json::json!({"title": "same", "body": "value"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("m".to_string()),
                serde_json::json!({"title": "same", "body": "value"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_stable_tie ORDER BY 1 ASC",
                vec![],
            )
            .await
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
fn should_project_function_columns() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_projection_mix";

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "hello world"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "other text"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, search_score(body, 'world') AS score FROM exec_projection_mix WHERE body LIKE '%world%' ORDER BY id ASC",
                vec![],
            )
            .await
            .expect("query should execute");

        // Assert
        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        match &result.rows[0][1] {
            Value::Float64(score) => assert!(*score > 0.0),
            _ => panic!("expected float score"),
        }
    });
}

#[test]
fn should_project_snippet_function_output_for_text_matches() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_snippet_output";

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "Rust enables fast query search"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'query') AS excerpt FROM exec_snippet_output WHERE title = 'alpha'",
                vec![],
            )
            .await
            .expect("snippet query should execute");

        // Assert
        assert_eq!(result.columns[0].name, "excerpt");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            Value::String(excerpt) => {
                assert_eq!(excerpt, "Rust enables fast <mark>query</mark> search");
            }
            _ => panic!("expected string snippet output"),
        }
    });
}

#[test]
fn should_order_by_pgvector_dot_operator() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_vector_dot_order";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [0.0, 2.0]}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_dot_order ORDER BY embedding <#> '[1,0]' ASC",
                vec![],
            )
            .await
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
        assert_eq!(
            ids,
            vec!["d2".to_string(), "d1".to_string(), "d3".to_string()]
        );
    });
}

#[test]
fn should_order_by_pgvector_l2_operator() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_vector_l2_order";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [0.0, 2.0]}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_l2_order ORDER BY embedding <-> '[1,0]' ASC",
                vec![],
            )
            .await
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
        assert_eq!(
            ids,
            vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]
        );
    });
}

#[test]
fn should_fail_query_when_vector_function_dimensions_mismatch() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_vector_mismatch";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 2.0]}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT vector_distance(embedding, '[1,0,0]') FROM exec_vector_mismatch",
                vec![],
            )
            .await;

        // Assert
        assert!(result.is_err());
    });
}

#[test]
fn should_order_by_hybrid_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_hybrid_order";

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("zeta".to_string()),
                serde_json::json!({"title": "doc1", "body": "red", "embedding": [10.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("alpha".to_string()),
                serde_json::json!({"title": "doc2", "body": "red", "embedding": [1.0, 0.0]}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM exec_hybrid_order ORDER BY score DESC",
                vec![],
            )
            .await
            .expect("query should execute");

        // Assert
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[1][0], Value::String("zeta".to_string()));

        let first_score = match &result.rows[0][1] {
            Value::Float64(value) => *value,
            _ => panic!("expected float score"),
        };
        let second_score = match &result.rows[1][1] {
            Value::Float64(value) => *value,
            _ => panic!("expected float score"),
        };
        assert!(first_score > second_score);
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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("z".to_string()),
                serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("a".to_string()),
                serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("m".to_string()),
                serde_json::json!({"body": "red", "embedding": [0.0, 1.0]}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS Score FROM exec_hybrid_alias_case ORDER BY SCORE DESC",
                vec![],
            )
            .await
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
fn should_filter_by_hybrid_score_threshold() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_hybrid_filter";

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "doc1", "body": "red apple", "embedding": [1.0, 0.0]}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "doc2", "body": "green apple", "embedding": [0.0, 2.0]}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_hybrid_filter WHERE hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) > 0.5",
                vec![],
            )
            .await
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    });
}
