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
use std::env;
use uuid::Uuid;

fn with_fallback() {
    env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-exec-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn openai_runtime_for_vectors() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env();
    config.embeddings = EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
        config: OpenAiConfig {
            api_key: "vector-tests".to_string(),
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
        },
        timeout_seconds: 1,
        max_batch_size: 1,
        max_retries: 1,
        base_url: Some("http://127.0.0.1:1".to_string()),
    });
    config
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
async fn should_fail_query_when_query_timeout_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.query_timeout_ms = 0;
    let path = data_dir("timeout");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_timeout";
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
    let result = cassie.execute_sql(&session, "SELECT title FROM exec_timeout", vec![]);

    // Assert
    let message = result
        .expect_err("query should fail when timeout is configured to 0")
        .to_string();
    assert!(
        message.contains("query timeout exceeded"),
        "expected timeout error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_fail_query_when_result_limit_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.max_result_rows = 1;
    let path = data_dir("max_rows");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_max_rows";
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
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-2".to_string()),
            serde_json::json!({"title": "beta"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "SELECT title FROM exec_max_rows ORDER BY title",
        vec![],
    );

    // Assert
    let message = result
        .expect_err("query should fail when row limit is configured too low")
        .to_string();
    assert!(
        message.contains("query result row limit exceeded"),
        "expected row limit error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_fail_query_when_cte_recursion_depth_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.cte_recursion_depth = 0;
    let path = data_dir("cte_depth");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_cte_depth";
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

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_depth WHERE n = 1 UNION ALL SELECT n FROM seq WHERE n = 1) SELECT n FROM seq",
                vec![],
            )
            ;

    // Assert
    let message = result
        .expect_err("recursive cte should fail when depth is exhausted")
        .to_string();
    assert!(
        message.contains("did not stabilize within 0 iterations"),
        "expected recursion depth error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_fail_query_when_temporary_spill_budget_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.temp_spill_budget_bytes = 16;
    let path = data_dir("spill_budget");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_spill";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "payload".to_string(),
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
            serde_json::json!({"payload": "very long payload data for spill budget test"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(&session, "SELECT payload FROM exec_spill", vec![]);

    // Assert
    let message = result
        .expect_err("query should fail when temp spill budget is exhausted")
        .to_string();
    assert!(
        message.contains("temporary storage budget exceeded"),
        "expected spill budget error, got {message}"
    );

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
            serde_json::json!({"embedding": [1.0, 0.0]}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"embedding": [0.0, 1.0]}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_score(embedding, '[1,0]') AS score FROM exec_vector_score_filter WHERE vector_score(embedding, '[1,0]') > 0.5",
            vec![],
        )

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
            serde_json::json!({"embedding": [1.0, 0.0]}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"embedding": [0.2, 0.0]}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"embedding": [10.0, 10.0]}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let params = vec![cassie::types::Value::String("[1,0]".to_string())];
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_vector_order_func ORDER BY vector_distance(embedding, $1) ASC",
            params,
        )
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
        vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]
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
            serde_json::json!({"title": "alpha", "body": "lorem world"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT search_score(body, 'world') AS score FROM exec_function_alias WHERE title = 'alpha'",
            vec![],
        )

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
fn should_apply_fulltext_index_params_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_fulltext_k1_b";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "bravo"}),
            )

            .unwrap();

        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_k1_b ON exec_fulltext_k1_b USING fulltext (body) WITH (k1 = 0, b = 0)",
                vec![],
            )

.unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_k1_b WHERE id = 'd1'",
                vec![],
            )
            .expect("query should execute");

        // Assert
        let expected = cassie::search::bm25::bm25_score(3.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0);
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "score");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            Value::Float64(score) => assert_eq!(*score, expected),
            _ => panic!("expected float score"),
        }
    });
}

#[test]
fn should_apply_fulltext_analyzer_stop_words_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_fulltext_analyzer_stop_words (id TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_fulltext_analyzer_stop_words (id, body) VALUES ('d1', 'the the alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_analyzer_stop_words ON exec_fulltext_analyzer_stop_words USING fulltext (body) WITH (analyzer = standard, stop_words = none)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'the') AS score FROM exec_fulltext_analyzer_stop_words",
                vec![],
            )
            .expect("query should execute");

        // Assert
        match &result.rows[0][0] {
            Value::Float64(score) => assert!(*score > 0.0),
            _ => panic!("expected float score"),
        }
    });
}

#[test]
fn should_reject_unknown_fulltext_analyzer() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_fulltext_bad_analyzer (body TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let err = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_bad_analyzer ON exec_fulltext_bad_analyzer USING fulltext (body) WITH (analyzer = unsupported)",
                vec![],
            )
            .expect_err("unknown analyzer should fail");

        // Assert
        assert!(err
            .to_string()
            .contains("unsupported fulltext analyzer 'unsupported'"));
    });
}

#[test]
fn should_reject_non_finite_fulltext_index_options_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_fulltext_non_finite";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();

        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_fulltext_non_finite".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1e999".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })

            .unwrap();

        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_non_finite WHERE id = 'd1'",
                vec![],
            )
            ;

        // Assert
        assert!(result.is_err());
    });
}

#[test]
fn should_reject_duplicate_fulltext_indexes_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_fulltext_duplicate";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();

        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_fulltext_duplicate_a".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1.2".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })

            .unwrap();
        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_fulltext_duplicate_b".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "2.0".to_string()),
                    ("k1".to_string(), "0.5".to_string()),
                    ("b".to_string(), "0.4".to_string()),
                ]),
            })

            .unwrap();

        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_duplicate WHERE id = 'd1'",
                vec![],
            )
            ;

        // Assert
        assert!(result.is_err());
    });
}

#[test]
fn should_allow_plain_select_with_non_finite_fulltext_metadata() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_plain_select_bad_fulltext";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_plain_select_bad_fulltext".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "inf".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })
            .unwrap();

        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_plain_select_bad_fulltext WHERE id = 'd1'",
                vec![],
            )
            .expect("plain select should execute");

        // Assert
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    });
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
                serde_json::json!({"title": "a"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "b"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "c"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d4".to_string()),
                serde_json::json!({"title": "d"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d5".to_string()),
                serde_json::json!({"title": "e"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_offset_limit ORDER BY title ASC LIMIT 2 OFFSET 2",
                vec![],
            )
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
                serde_json::json!({"title": "c"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "a"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "b"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let default_offset_result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1",
                vec![],
            )
            .expect("query should execute");

        let explicit_offset_result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1 OFFSET 0",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(default_offset_result.rows.len(), 1);
        assert_eq!(explicit_offset_result.rows.len(), 1);
        assert_eq!(default_offset_result.rows, explicit_offset_result.rows);
    });
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
                serde_json::json!({"title": "alpha", "body": "hello world"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "other text"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, search_score(body, 'world') AS score FROM exec_projection_mix WHERE body LIKE '%world%' ORDER BY id ASC",
                vec![],
            )

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
                serde_json::json!({"title": "alpha", "body": "Rust enables fast query search"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'query') AS excerpt FROM exec_snippet_output WHERE title = 'alpha'",
                vec![],
            )

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
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [0.0, 2.0]}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_dot_order ORDER BY embedding <#> '[1,0]' ASC",
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
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [0.0, 2.0]}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_l2_order ORDER BY embedding <-> '[1,0]' ASC",
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
                serde_json::json!({"embedding": [1.0, 2.0]}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie.execute_sql(
            &session,
            "SELECT vector_distance(embedding, '[1,0,0]') FROM exec_vector_mismatch",
            vec![],
        );

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
                Some("zeta".to_string()),
                serde_json::json!({"title": "doc1", "body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("alpha".to_string()),
                serde_json::json!({"title": "doc2", "body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM exec_hybrid_order ORDER BY score DESC",
                vec![],
            )

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
                serde_json::json!({"title": "doc1", "body": "red apple", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "doc2", "body": "green apple", "embedding": [0.0, 2.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_hybrid_filter WHERE hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) > 0.5",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    });
}

#[test]
fn should_reject_hybrid_score_with_wrong_arity() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_hybrid_wrong_arity";

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
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie.execute_sql(
            &session,
            "SELECT hybrid_score(search_score(body, 'red')) FROM exec_hybrid_wrong_arity",
            vec![],
        );

        // Assert
        let error = result.expect_err("query should reject wrong arity");
        assert!(error.to_string().contains("hybrid_score"));
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
fn should_fail_unknown_function_during_execution() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new().unwrap();
        let collection = "exec_unknown_function";

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

        let logical = LogicalPlan {
            command: None,
            source: QuerySource::Collection(collection.to_string()),
            collection: collection.to_string(),
            ctes: vec![],
            distinct: false,
            distinct_on: Vec::new(),
            projection: vec![SelectItem::Function {
                function: FunctionCall {
                    name: "unknown_fn".to_string(),
                    args: vec![Expr::Column("title".to_string())],
                },
                alias: Some("score".to_string()),
            }],
            filter: None,
            group_by: vec![],
            having: None,
            order: vec![],
            limit: Some(10),
            offset: Some(0),
            set: None,
        };

        let physical = PhysicalPlan {
            collection: logical.collection.clone(),
            operators: vec![cassie::planner::physical::Operator::Project],
            estimates: Default::default(),
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
            scan_limit: None,
            selected_index: None,
            covered_index: false,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
            logical,
        };

        // Act
        let result = executor::run(&cassie, physical, vec![]);

        // Assert
        assert!(result.is_err());
    });
}

#[tokio::test]
async fn should_execute_create_alter_and_drop_table_commands() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_command");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let session = cassie.create_session("tester", None);
    let table_name = "ddl_table";

    // Act
    let create = cassie
        .execute_sql(
            &session,
            "CREATE TABLE ddl_table (id TEXT, title TEXT)",
            vec![],
        )
        .unwrap();
    assert_eq!(create.command, "CREATE TABLE");
    assert_eq!(create.columns.len(), 0);
    assert!(cassie.catalog.exists(table_name));

    cassie
        .midge
        .put_document(
            table_name,
            Some("d1".to_string()),
            serde_json::json!({"id": "d1", "title": "alpha"}),
        )
        .unwrap();

    let alter_add = cassie
        .execute_sql(
            &session,
            "ALTER TABLE ddl_table ADD COLUMN status TEXT",
            vec![],
        )
        .unwrap();
    let alter_rename = cassie
        .execute_sql(
            &session,
            "ALTER TABLE ddl_table RENAME TO ddl_table_archive",
            vec![],
        )
        .unwrap();
    let rename_rows = cassie
        .execute_sql(
            &session,
            "SELECT id, status FROM ddl_table_archive ORDER BY id",
            vec![],
        )
        .unwrap();
    let drop = cassie
        .execute_sql(&session, "DROP TABLE ddl_table_archive", vec![])
        .unwrap();

    // Assert
    assert_eq!(alter_add.command, "ALTER TABLE");
    assert_eq!(alter_rename.command, "ALTER TABLE");
    assert!(!cassie.catalog.exists(table_name));
    assert_eq!(rename_rows.columns.len(), 2);
    assert_eq!(rename_rows.rows.len(), 1);
    assert_eq!(rename_rows.rows[0][0], Value::String("d1".to_string()));
    assert_eq!(drop.command, "DROP TABLE");
    assert!(!cassie.catalog.exists("ddl_table_archive"));

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_execute_alter_table_rename_column_command() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_rename_column_command");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let session = cassie.create_session("tester", None);

    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rename_column_docs (id TEXT, title TEXT)",
            vec![],
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            "rename_column_docs",
            Some("d1".to_string()),
            serde_json::json!({"id": "d1", "title": "alpha"}),
        )
        .unwrap();

    // Act
    let rename = cassie
        .execute_sql(
            &session,
            "ALTER TABLE rename_column_docs RENAME COLUMN title TO headline",
            vec![],
        )
        .unwrap();
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, headline FROM rename_column_docs ORDER BY id",
            vec![],
        )
        .unwrap();

    let schema = cassie
        .catalog
        .get_schema("rename_column_docs")
        .expect("schema should exist");

    // Assert
    assert_eq!(rename.command, "ALTER TABLE");
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0][0], Value::String("d1".to_string()));
    assert_eq!(rows.rows[0][1], Value::String("alpha".to_string()));
    assert!(schema.fields.iter().any(|field| field.name == "headline"));
    assert!(!schema.fields.iter().any(|field| field.name == "title"));

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_execute_create_and_drop_index_commands() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_index_command");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let session = cassie.create_session("tester", None);

    // Act
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE idx_commands (id TEXT, title TEXT)",
            vec![],
        )
        .unwrap();

    let create_index = cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_title ON idx_commands USING btree (title)",
            vec![],
        )
        .unwrap();

    let catalog_index = cassie
        .catalog
        .get_index("idx_commands", "idx_title")
        .expect("index should be in catalog");
    let stored_index = cassie
        .midge
        .get_index("idx_commands", "idx_title")
        .unwrap()
        .expect("index should be persisted");

    let drop_index = cassie
        .execute_sql(
            &session,
            "DROP INDEX IF EXISTS idx_title ON idx_commands",
            vec![],
        )
        .unwrap();

    // Assert
    assert_eq!(create_index.command, "CREATE INDEX");
    assert_eq!(create_index.columns.len(), 0);
    assert!(!catalog_index.unique);
    assert_eq!(catalog_index.field, "title");
    assert_eq!(stored_index.field, "title");
    assert_eq!(drop_index.command, "DROP INDEX");
    assert!(cassie
        .catalog
        .get_index("idx_commands", "idx_title")
        .is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_execute_create_composite_index_command() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_composite_index_command");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let session = cassie.create_session("tester", None);

    cassie
        .execute_sql(
            &session,
            "CREATE TABLE composite_index_docs (id TEXT, title TEXT, score INT)",
            vec![],
        )
        .unwrap();

    // Act
    let create_index = cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_title_score ON composite_index_docs USING btree (title, score)",
            vec![],
        )
        .unwrap();

    let catalog_index = cassie
        .catalog
        .get_index("composite_index_docs", "idx_title_score")
        .expect("index should be in catalog");
    let stored_index = cassie
        .midge
        .get_index("composite_index_docs", "idx_title_score")
        .unwrap()
        .expect("index should be persisted");

    // Assert
    assert_eq!(create_index.command, "CREATE INDEX");
    assert_eq!(create_index.columns.len(), 0);
    assert_eq!(
        catalog_index.fields,
        vec!["title".to_string(), "score".to_string()]
    );
    assert_eq!(
        stored_index.fields,
        vec!["title".to_string(), "score".to_string()]
    );
    assert_eq!(catalog_index.field, "title");
    assert_eq!(stored_index.field, "title");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_create_vector_index_command() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_vector_index_create_command");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_commands (id TEXT, content TEXT, embedding VECTOR(1536))",
                vec![],
            )

.unwrap();

        let create_index = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_embedding ON idx_vector_commands USING vector (embedding) WITH (source_field = content, metric = l2)",
                vec![],
            )
            .unwrap();

        let catalog_index = cassie
            .catalog
            .get_index("idx_vector_commands", "idx_vector_embedding")
            .expect("index should be in catalog");
        let stored_vector = cassie
            .midge
            .get_vector_index("idx_vector_commands", "embedding")

            .unwrap()
            .expect("vector index should be persisted");

        // Assert
        assert_eq!(create_index.command, "CREATE INDEX");
        assert_eq!(create_index.columns.len(), 0);
        assert!(matches!(catalog_index.kind, IndexKind::Vector));
        assert_eq!(catalog_index.field, "embedding");
        assert_eq!(catalog_index.fields, vec!["embedding".to_string()]);
        assert_eq!(
            catalog_index.options.get("source_field"),
            Some(&"content".to_string())
        );
        assert_eq!(catalog_index.options.get("metric"), Some(&"l2".to_string()));
        assert_eq!(stored_vector.field, "embedding");
        assert_eq!(stored_vector.source_field, "content");
        assert_eq!(stored_vector.metadata.metric, DistanceMetric::L2);
        assert_eq!(
            stored_vector.metadata.provider,
            cassie.embedding_provider.provider_name()
        );
        assert_eq!(
            stored_vector.metadata.model,
            cassie.embedding_provider.model_name().to_string()
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_drop_vector_index_command() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_vector_index_drop_command");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);

        // Arrange
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_commands (id TEXT, content TEXT, embedding VECTOR(1536))",
                vec![],
            )

.unwrap();

        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_embedding ON idx_vector_commands USING vector (embedding) WITH (source_field = content, metric = l2)",
                vec![],
            )
            .unwrap();

        // Act
        let drop_index = cassie
            .execute_sql(
                &session,
                "DROP INDEX idx_vector_embedding ON idx_vector_commands",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(drop_index.command, "DROP INDEX");
        assert!(cassie
            .catalog
            .get_index("idx_vector_commands", "idx_vector_embedding")
            .is_none());
        assert!(cassie
            .midge
            .get_vector_index("idx_vector_commands", "embedding")

            .unwrap()
            .is_none());
    });

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_execute_create_function_and_evaluate_user_body() {
    // Arrange
    with_fallback();
    let path = data_dir("create_function_exec");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let session = cassie.create_session("tester", None);

    let collection = "udf_eval";

    cassie
        .execute_sql(&session, "CREATE TABLE udf_eval (id TEXT, x INT)", vec![])
        .unwrap();
    cassie.register_collection(
        collection,
        vec![
            ("id".to_string(), DataType::Text),
            ("x".to_string(), DataType::Int),
        ]
        .into_iter()
        .collect(),
    );

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"id": "d1", "x": 3}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"id": "d2", "x": 7}),
        )
        .unwrap();

    cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION double_input(x INT) RETURNS INT AS \"x\"",
            vec![],
        )
        .unwrap();

    let query = cassie
        .execute_sql(
            &session,
            "SELECT id, double_input(x) AS doubled FROM udf_eval ORDER BY id ASC",
            vec![],
        )
        .unwrap();

    // Assert
    let function = cassie
        .catalog
        .get_function("double_input")
        .expect("function should be registered");
    assert_eq!(function.name, "double_input");
    assert_eq!(query.columns[1].name, "doubled");
    assert_eq!(
        query.rows[0],
        vec![Value::String("d1".to_string()), Value::Int64(3),]
    );
    assert_eq!(
        query.rows[1],
        vec![Value::String("d2".to_string()), Value::Int64(7),]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_execute_drop_function_and_reject_subsequent_use() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_function_exec");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let session = cassie.create_session("tester", None);

    let collection = "udf_drop";

    cassie
        .execute_sql(&session, "CREATE TABLE udf_drop (id TEXT, x INT)", vec![])
        .unwrap();
    cassie.register_collection(
        collection,
        vec![
            ("id".to_string(), DataType::Text),
            ("x".to_string(), DataType::Int),
        ]
        .into_iter()
        .collect(),
    );

    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"id": "d1", "x": 3}),
        )
        .unwrap();

    cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION square(x INT) RETURNS INT AS \"x\"",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(&session, "DROP FUNCTION square", vec![])
        .unwrap();

    let result = cassie.execute_sql(&session, "SELECT square(x) FROM udf_drop", vec![]);
    let missing = cassie.catalog.get_function("square").is_none();

    // Assert
    assert!(missing);
    assert!(result.is_err());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_procedure_body_with_arguments_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("procedure_exec");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(&session, "CREATE TABLE procedure_exec (title TEXT)", vec![])

.unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE PROCEDURE store_title(title TEXT) AS "INSERT INTO procedure_exec (title) VALUES ($1)""#,
                vec![],
            )
            .unwrap();

        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let call = restarted
            .execute_sql(&session, "CALL store_title('alpha')", vec![])

.unwrap();
        let rows = restarted
            .execute_sql(
                &session,
                "SELECT title FROM procedure_exec ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(call.command, "CALL");
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_procedure_bodies_with_transaction_control() {
    // Arrange
    with_fallback();
    let path = data_dir("procedure_transaction_control");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
            &session,
            r#"CREATE PROCEDURE stop_here() AS "BEGIN""#,
            vec![],
        );

        // Assert
        let error = result.expect_err("procedure creation should fail");
        assert!(error
            .to_string()
            .contains("transaction control statements inside procedures"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_recursive_procedure_calls() {
    // Arrange
    with_fallback();
    let path = data_dir("procedure_recursion");
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
                r#"CREATE PROCEDURE loop_a() AS "CALL loop_b()""#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE PROCEDURE loop_b() AS "CALL loop_a()""#,
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie.execute_sql(&session, "CALL loop_a()", vec![]);
        // Assert
        let error = result.expect_err("recursive call should fail");
        assert!(error.to_string().contains("recursively invoked"));

        let _ = std::fs::remove_dir_all(path);
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
