use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-sql-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

fn put_legacy_document(cassie: &Cassie, collection: &str, id: &str, payload: serde_json::Value) {
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(
        format!("doc:{collection}:{id}").into_bytes(),
        payload.to_string().into_bytes(),
        None,
    )
    .unwrap();
    tx.commit(WriteOptions::sync()).unwrap();
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

#[test]
fn should_execute_insert_values_with_explicit_columns_returning_columns() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_returning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_returning (title TEXT, body TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_returning (title, body) VALUES ('alpha', 'first') RETURNING title, body",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "INSERT 0 1");
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.columns[1].name, "body");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[0][1], Value::String("first".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_insert_values_using_table_column_order() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_table_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_table_order (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_table_order VALUES ('alpha', 7)",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM insert_values_table_order",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(selected.rows[0][1], Value::Int64(7));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_generated_row_id_from_insert_values() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_id (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_id (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.columns[0].name, "_id");
        assert_eq!(inserted.rows.len(), 1);
        assert!(matches!(&inserted.rows[0][0], Value::String(id) if !id.is_empty()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_when_not_null_constraint_fails() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_not_null (title TEXT NOT NULL)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_not_null (title) VALUES (NULL)",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_default_values_for_insert_values() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_defaults (id) VALUES (1) RETURNING status",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.rows.len(), 1);
        assert_eq!(inserted.rows[0][0], Value::String("pending".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_when_vector_dimensions_mismatch() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_vector_dimensions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_vector_dimensions (embedding VECTOR(2))",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_vector_dimensions (embedding) VALUES ($1)",
                vec![Value::Vector(Vector::new(vec![1.0]))],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("expects vector(2)"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_store_insert_values_as_row_blobs() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_row_blob");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_row_blob (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_row_blob (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        let row_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"r/insert_values_row_blob/")
            .await
            .unwrap();
        let legacy_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:insert_values_row_blob:")
            .await
            .unwrap();

        // Assert
        assert_eq!(row_entries.len(), 1);
        assert!(legacy_entries.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_insert_select_with_returning_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_select_returning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_source (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_target (name TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_source (title, score) VALUES ('banana', 2)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_source (title, score) VALUES ('apple', 1)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_target (name, score) SELECT title, score FROM insert_select_source ORDER BY title ASC RETURNING name, score",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.command, "INSERT 0 2");
        assert_eq!(inserted.rows.len(), 2);
        assert_eq!(inserted.rows[0][0], Value::String("apple".to_string()));
        assert_eq!(inserted.rows[0][1], Value::Int64(1));
        assert_eq!(inserted.rows[1][0], Value::String("banana".to_string()));
        assert_eq!(inserted.rows[1][1], Value::Int64(2));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_select_shape_mismatch_before_writing() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_select_shape");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_shape_source (title TEXT, body TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_shape_target (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_shape_source (title, body) VALUES ('alpha', 'first')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_shape_target (title) SELECT title, body FROM insert_select_shape_source",
                vec![],
            )
            .await;
        let target_rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM insert_select_shape_target",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("column/value counts mismatch"));
        assert!(target_rows.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_default_values_for_insert_select() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_select_defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_default_source (source_id INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_default_target (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_default_source (source_id) VALUES (1)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_default_target (id) SELECT source_id FROM insert_select_default_source RETURNING status",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.rows.len(), 1);
        assert_eq!(inserted.rows[0][0], Value::String("pending".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_update_where_returning_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("update_where_returning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_where_returning (title TEXT, status TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_where_returning (title, status) VALUES ('alpha', 'old')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_where_returning (title, status) VALUES ('beta', 'old')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_where_returning SET status = 'done' WHERE title = 'alpha' RETURNING _id, title, status",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, status FROM update_where_returning ORDER BY title ASC",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(updated.command, "UPDATE 1");
        assert_eq!(updated.rows.len(), 1);
        assert!(matches!(&updated.rows[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(updated.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(updated.rows[0][2], Value::String("done".to_string()));
        assert_eq!(selected.rows[0][1], Value::String("done".to_string()));
        assert_eq!(selected.rows[1][1], Value::String("old".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_row_id_when_update_rewrites_row_blob() {
    // Arrange
    with_fallback();
    let path = data_dir("update_preserve_id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_preserve_id (title TEXT, body TEXT)",
                vec![],
            )
            .await
            .unwrap();
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO update_preserve_id (title, body) VALUES ('alpha', 'old') RETURNING _id",
                vec![],
            )
            .await
            .unwrap();
        let original_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_preserve_id SET body = 'new' WHERE title = 'alpha' RETURNING _id",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(updated.rows[0][0], Value::String(original_id));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_validation_failure_without_mutating_row() {
    // Arrange
    with_fallback();
    let path = data_dir("update_validation_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_validation_failure (title TEXT NOT NULL)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_validation_failure (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_validation_failure SET title = NULL RETURNING title",
                vec![],
            )
            .await;
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM update_validation_failure",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(updated.is_err());
        assert!(updated.unwrap_err().to_string().contains("cannot be null"));
        assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_delete_where_returning_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_where_returning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE delete_where_returning (title TEXT, status TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_where_returning (title, status) VALUES ('alpha', 'old')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_where_returning (title, status) VALUES ('beta', 'old')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_where_returning WHERE title = 'alpha' RETURNING _id, title",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM delete_where_returning ORDER BY title ASC",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(deleted.command, "DELETE 1");
        assert_eq!(deleted.rows.len(), 1);
        assert!(matches!(&deleted.rows[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(deleted.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_zero_rows_for_delete_without_matches() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_no_match");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE delete_no_match (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_no_match (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_no_match WHERE title = 'missing' RETURNING title",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(&session, "SELECT title FROM delete_no_match", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(deleted.command, "DELETE 0");
        assert!(deleted.rows.is_empty());
        assert_eq!(selected.rows.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_delete_legacy_fallback_key_for_sql_delete() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_legacy_cleanup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE delete_legacy_cleanup (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_legacy_cleanup (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .await
            .unwrap();
        let row_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };
        put_legacy_document(
            &cassie,
            "delete_legacy_cleanup",
            &row_id,
            serde_json::json!({"title": "stale"}),
        );

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_legacy_cleanup WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();
        let deleted = cassie
            .midge
            .get_document("delete_legacy_cleanup", &row_id)
            .await
            .unwrap();

        // Assert
        assert!(deleted.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_transition_session_state_for_transaction_control() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;

        // Act
        let begin = cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        let during = session.transaction_status().await;
        let commit = cassie
            .execute_sql(&session, "COMMIT", vec![])
            .await
            .unwrap();
        let after = session.transaction_status().await;

        // Assert
        assert_eq!(begin.command, "BEGIN");
        assert_eq!(during, "in_transaction");
        assert_eq!(commit.command, "COMMIT");
        assert_eq!(after, "idle");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_restore_idle_state_on_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();

        // Act
        let rollback = cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .await
            .unwrap();
        let after = session.transaction_status().await;

        // Assert
        assert_eq!(rollback.command, "ROLLBACK");
        assert_eq!(after, "idle");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_autocommit_writes_visible_after_success() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_autocommit");
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
                "CREATE TABLE transaction_autocommit (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_autocommit (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(&session, "SELECT title FROM transaction_autocommit", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(session.transaction_status().await, "idle");
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_unsupported_transaction_control_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_unsupported");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;

        // Act
        let savepoint = cassie.execute_sql(&session, "SAVEPOINT sp", vec![]).await;

        // Assert
        assert!(savepoint.is_err());
        assert!(savepoint.unwrap_err().to_string().contains("unsupported"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_advisory_lock_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_advisory_lock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_advisory_lock (id INT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let lock = cassie
            .execute_sql(
                &session,
                "SELECT pg_advisory_lock(1) FROM transaction_advisory_lock",
                vec![],
            )
            .await;

        // Assert
        assert!(lock.is_err());
        assert!(lock
            .unwrap_err()
            .to_string()
            .contains("unsupported function"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_writes_after_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_rollback_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_rollback_writes (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_rollback_writes (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_rollback_writes",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hide_transaction_writes_from_other_sessions_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_uncommitted_visibility");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let writer = cassie.create_session("writer", None).await;
        let reader = cassie.create_session("reader", None).await;
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE transaction_uncommitted_visibility (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&writer, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO transaction_uncommitted_visibility (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &reader,
                "SELECT title FROM transaction_uncommitted_visibility",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_writes_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_read_your_writes (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_read_your_writes",
                vec![],
            )
            .await
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
fn should_persist_transaction_writes_after_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_commit_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let writer = cassie.create_session("writer", None).await;
        let reader = cassie.create_session("reader", None).await;
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE transaction_commit_writes (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&writer, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO transaction_commit_writes (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        cassie.execute_sql(&writer, "COMMIT", vec![]).await.unwrap();
        let selected = cassie
            .execute_sql(
                &reader,
                "SELECT title FROM transaction_commit_writes",
                vec![],
            )
            .await
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
fn should_keep_transaction_insert_out_of_storage_until_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_storage_routing");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_storage_routing (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_storage_routing (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .await
            .unwrap();
        let row_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };
        let before_commit = cassie
            .midge
            .get_document("transaction_storage_routing", &row_id)
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .await
            .unwrap();
        let after_commit = cassie
            .midge
            .get_document("transaction_storage_routing", &row_id)
            .await
            .unwrap();

        // Assert
        assert!(before_commit.is_none());
        assert_eq!(
            after_commit.unwrap().payload["title"],
            serde_json::Value::String("alpha".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_work_after_transaction_error_until_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_failed_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_failed_state (title TEXT NOT NULL)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        let failed_insert = cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_failed_state (title) VALUES (NULL)",
                vec![],
            )
            .await;
        assert!(failed_insert.is_err());

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_failed_state",
                vec![],
            )
            .await;

        // Assert
        assert!(selected.is_err());
        assert!(selected
            .unwrap_err()
            .to_string()
            .contains("rollback required"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_allow_work_after_failed_transaction_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_failed_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_failed_recovery (title TEXT NOT NULL)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        let failed_insert = cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_failed_recovery (title) VALUES (NULL)",
                vec![],
            )
            .await;
        assert!(failed_insert.is_err());
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_failed_recovery",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_update_after_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_update_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_update_rollback (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_rollback SET title = 'beta'",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_update_rollback",
                vec![],
            )
            .await
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
fn should_discard_transaction_delete_after_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_delete_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_delete_rollback (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_delete_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_delete_rollback WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_delete_rollback",
                vec![],
            )
            .await
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
fn should_read_own_transaction_update_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_update_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_update_read_your_writes (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_read_your_writes SET title = 'beta'",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_update_read_your_writes",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_delete_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_delete_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_delete_read_your_writes (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_delete_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_delete_read_your_writes WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_delete_read_your_writes",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_is_null_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_is_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_is_null (title TEXT, archived_at TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_null (title, archived_at) VALUES ('alpha', NULL)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_is_null WHERE archived_at IS NULL",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("predicate_is_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_is_not_null (title TEXT, archived_at TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('alpha', NULL)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_is_not_null WHERE archived_at IS NOT NULL",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_in_list_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_in_list");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_in_list (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_in_list (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_in_list (title) VALUES ('gamma')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_in_list WHERE title IN ('alpha', 'beta')",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("predicate_not_in_list");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_in_list (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_in_list (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_in_list (title) VALUES ('gamma')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_in_list WHERE title NOT IN ('alpha', 'beta')",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("predicate_between");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_between (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_between (title, score) VALUES ('alpha', 5)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_between (title, score) VALUES ('beta', 15)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_between WHERE score BETWEEN 10 AND 20",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_between_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not_between");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_between (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_between (title, score) VALUES ('alpha', 5)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_between (title, score) VALUES ('beta', 15)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_between WHERE score NOT BETWEEN 10 AND 20",
                vec![],
            )
            .await
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
fn should_filter_rows_with_cast_function_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_cast_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_cast_function (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_cast_function (title, score) VALUES ('alpha', 10)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_cast_function WHERE CAST(score AS TEXT) = '10'",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("predicate_pg_cast");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_pg_cast (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_pg_cast (title, score) VALUES ('alpha', 10)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_pg_cast WHERE score::TEXT = '10'",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("order_nulls_first");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE order_nulls_first (title TEXT, archived_at TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_first (title, archived_at) VALUES ('alpha', 'today')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_first (title, archived_at) VALUES ('beta', NULL)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM order_nulls_first ORDER BY archived_at NULLS FIRST",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("order_nulls_last");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE order_nulls_last (title TEXT, archived_at TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_last (title, archived_at) VALUES ('alpha', 'today')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_last (title, archived_at) VALUES ('beta', NULL)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM order_nulls_last ORDER BY archived_at NULLS LAST",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("predicate_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_outer (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_inner (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_inner (title) VALUES ('present')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_exists_outer WHERE EXISTS (SELECT title FROM predicate_exists_inner)",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_empty_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_empty_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_outer (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_inner (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_empty_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_empty_exists_outer WHERE EXISTS (SELECT title FROM predicate_empty_exists_inner)",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_inner_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_inner");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE join_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO join_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO join_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT join_users.name, join_orders.total FROM join_users JOIN join_orders ON join_users.user_key = join_orders.order_user_key",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("join_left");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE left_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE left_orders (order_user_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO left_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT left_users.name, left_orders.total FROM left_users LEFT JOIN left_orders ON left_users.user_key = left_orders.order_user_key",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("from_subquery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None).await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE from_subquery_docs (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO from_subquery_docs (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT recent.title FROM (SELECT title FROM from_subquery_docs) AS recent",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
