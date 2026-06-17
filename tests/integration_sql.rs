use cassie::app::Cassie;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-sql-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

#[test]
fn should_execute_sql_query_after_catalog_hydration() {
    // Arrange
    with_fallback();
    let path = data_dir("restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();

        let collection = "sql_hydration";
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
            .create_collection(collection, schema)
            .await
            .unwrap();
        let _ = cassie
            .midge
            .put_document(
                collection,
                None,
                serde_json::json!({"title": "sql", "body": "hybrid path"}),
            )
            .await
            .unwrap();

        // Act
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().await.unwrap();
        let session = restarted.create_session("tester", None).await;
        let result = restarted
            .execute_sql(
                &session,
                "SELECT title FROM sql_hydration WHERE title = 'sql'",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.columns[0].name, "title");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_limit_offset_after_ordering() {
    // Arrange
    with_fallback();
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
                serde_json::json!({"title": "pear", "body": "c"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "apple", "body": "a"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "banana", "body": "b"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_limit_offset_order ORDER BY title ASC LIMIT 2 OFFSET 1",
                vec![],
            )
            .await
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
fn should_execute_sql_with_non_recursive_cte() {
    // Arrange
    with_fallback();
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
                serde_json::json!({"title": "alpha", "body": "hello"}),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "world"}),
            )
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "WITH docs_cte AS (SELECT title FROM integration_cte WHERE title = 'alpha') SELECT title FROM docs_cte",
                vec![],
            )
            .await
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
    with_fallback();
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
            .put_document(collection, Some("d1".to_string()), serde_json::json!({"n": 1}))
            .await
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE counter(n) AS (SELECT n FROM integration_recursive_cte WHERE n = 1 UNION ALL SELECT n FROM counter WHERE n = 1) SELECT n FROM counter ORDER BY n",
            vec![],
            )
            .await
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
        assert_eq!(rows, vec![1]);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_namespace_on_create_schema() {
    // Arrange
    with_fallback();
    let path = data_dir("create_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie
            .midge
            .ensure_families_ready()
            .expect("families ready");

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA analytics", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "CREATE SCHEMA");
        assert!(cassie.catalog.namespace_exists("analytics").await);
        assert!(cassie
            .midge
            .list_namespaces()
            .await
            .iter()
            .any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_enforce_constraints_during_ingest() {
    // Arrange
    with_fallback();
    let path = data_dir("constraints_ingest");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;

        // Act
        let create = cassie
            .execute_sql(
                &session,
                "CREATE TABLE constraint_docs (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE, status TEXT DEFAULT 'pending', score INT CHECK (score >= 18))",
                vec![],
            )
            .await
            .unwrap();

        let first = cassie
            .ingest_document(
                "constraint_docs",
                serde_json::json!({"id": 1, "email": "a@example.com", "score": 25}),
            )
            .await
            .unwrap();
        let missing_not_null = cassie
            .ingest_document("constraint_docs", serde_json::json!({"id": 2, "score": 20}))
            .await;
        let duplicate = cassie
            .ingest_document(
                "constraint_docs",
                serde_json::json!({"id": 3, "email": "a@example.com", "score": 19}),
            )
            .await;
        let rejected_check = cassie
            .ingest_document(
                "constraint_docs",
                serde_json::json!({"id": 4, "email": "b@example.com", "score": 17}),
            )
            .await;

        let inserted = cassie
            .midge
            .get_document("constraint_docs", &first)
            .await
            .unwrap()
            .expect("document inserted");

        // Assert
        assert_eq!(create.command, "CREATE TABLE");
        assert_eq!(
            inserted.payload.get("status").expect("status is defaulted"),
            &serde_json::Value::String("pending".to_string())
        );
        assert!(missing_not_null.is_err());
        assert!(missing_not_null
            .unwrap_err()
            .to_string()
            .contains("cannot be null"));
        assert!(duplicate.is_err());
        assert!(duplicate
            .unwrap_err()
            .to_string()
            .contains("unique constraint"));
        assert!(rejected_check.is_err());
        assert!(rejected_check
            .unwrap_err()
            .to_string()
            .contains("check constraint"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_collection_constraints_on_startup() {
    // Arrange
    with_fallback();
    let path = data_dir("constraints_hydrate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE hydrated_constraints (id INT, email TEXT NOT NULL UNIQUE, score INT CHECK (score >= 0))",
                vec![],
            )
            .await
            .unwrap();

        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().await.unwrap();

        let constraints = restarted.catalog.get_constraints("hydrated_constraints").await;
        // Assert
        assert_eq!(constraints.len(), 2);
        assert!(constraints.iter().any(|constraint| constraint.not_null));
        assert!(constraints.iter().any(|constraint| constraint.unique));
        assert!(constraints.iter().any(|constraint| constraint.check.is_some()));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_ignore_duplicate_create_schema_when_if_not_exists_is_set() {
    // Arrange
    with_fallback();
    let path = data_dir("create_schema_if_not_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.create_namespace("analytics").await.unwrap();

        let initial = cassie.midge.list_namespaces().await;

        // Act
        let session = cassie.create_session("tester", None).await;
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA IF NOT EXISTS analytics", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "CREATE SCHEMA");
        let namespaced = cassie.midge.list_namespaces().await;
        assert_eq!(namespaced.len(), initial.len());
        assert!(namespaced.iter().any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}
