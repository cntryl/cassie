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

fn seed_alias_query_docs(cassie: &Cassie, collection: &str) {
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

    for (title, body, embedding) in [
        ("alpha", "world news", [1.0, 2.0]),
        ("beta", "world peace", [0.5, 1.5]),
        ("gamma", "misc", [2.0, 0.5]),
    ] {
        cassie
            .midge
            .put_document(
                collection,
                None,
                serde_json::json!({
                    "title": title,
                    "body": body,
                    "embedding": embedding,
                }),
            )
            .unwrap();
    }
}

fn assert_alias_query_result(result: executor::QueryResult) {
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
}

fn assert_parameterized_alias_result(result: &executor::QueryResult) {
    assert_eq!(result.rows.len(), 1);
    match &result.rows[0][0] {
        Value::String(value) => assert_eq!(value, "alpha"),
        _ => panic!("expected string in first column"),
    }
}

#[test]
fn should_execute_query_with_alias_filters() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-exec-{}", Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "exec_docs_alias";
    seed_alias_query_docs(&cassie, collection);

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT title AS title_out, search_score(body, 'world') AS score FROM exec_docs_alias WHERE body LIKE '%world%' OR title = 'gamma' ORDER BY score DESC, id ASC LIMIT 2",
            vec![],
        )
        .expect("query should execute");
    assert_alias_query_result(result);

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
    assert_parameterized_alias_result(&param_result);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_query_respects_boolean_precedence() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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

#[test]
fn should_execute_query_parentheses_override_precedence() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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
fn should_execute_query_with_non_recursive_cte() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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

#[test]
fn should_execute_query_with_ordered_cte_dependencies() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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

#[test]
fn should_execute_query_passes_params_to_cte_main_query() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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

#[test]
fn should_execute_recursive_cte_until_stabilization() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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
            "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_recursive WHERE n = 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
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
    assert_eq!(values, vec![1, 2]);
}

#[test]
fn should_execute_recursive_cte_enforces_depth_limit_when_no_stabilization() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
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
