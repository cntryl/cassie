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
            operator_feedback: Default::default(),
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
            scan_limit: None,
            selected_index: None,
            covered_index: false,
            column_batch_index: None,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
            parallel_aggregate_candidate: false,
            aggregate_acceleration: false,
            access_path: cassie::planner::physical::ReadAccessPath::CollectionScan,
            access_path_reason: "command-path".to_string(),
            fallback_reason: Some("command".to_string()),
            pagination_strategy: cassie::planner::physical::PaginationStrategy::None,
            top_k_mode: cassie::planner::physical::TopKMode::None,
            early_stop: cassie::planner::physical::EarlyStopMode::None,
            projection_shape: cassie::planner::physical::ProjectionShape::Other,
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
