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

        cassie.midge.create_collection(collection, schema).unwrap();
        let _ = cassie
            .midge
            .put_document(
                collection,
                None,
                serde_json::json!({"title": "sql", "body": "hybrid path"}),
            )
            .unwrap();

        // Act
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().await.unwrap();
        let session = restarted.create_session("tester", None);
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
fn should_order_column_top_k_with_deterministic_tie_break() {
    // Arrange
    with_fallback();
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
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_filtered_ordered_column_query_without_changing_results() {
    // Arrange
    with_fallback();
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
            ).await;

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

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_projected_scan_range_query_without_changing_results() {
    // Arrange
    with_fallback();
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
            .await
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
    with_fallback();
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
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[0][1], Value::String("beta".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_function_projection_query_without_changing_results() {
    // Arrange
    with_fallback();
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
            ).await;

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

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("ALPHA".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_text_scalar_functions_query() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_text_functions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_text_functions (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_text_functions (title) VALUES ('  Alpha  ')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT lower(title) AS lowered, upper(title) AS raised, length(title) AS chars, substring(title, 3, 5) AS slice, trim(title) AS trimmed, concat(trim(title), '-done') AS combined FROM scalar_text_functions",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("  alpha  ".to_string()),
                Value::String("  ALPHA  ".to_string()),
                Value::Int64(9),
                Value::String("Alpha".to_string()),
                Value::String("Alpha".to_string()),
                Value::String("Alpha-done".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_coalesce_scalar_function_query() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_coalesce_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_coalesce_function (title TEXT, fallback TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_coalesce_function (title, fallback) VALUES (NULL, 'backup')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT coalesce(title, fallback, 'missing') AS value FROM scalar_coalesce_function",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("scalar_numeric_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_numeric_function (delta INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_numeric_function (delta) VALUES (-42)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT abs(delta) AS magnitude FROM scalar_numeric_function",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::Int64(42)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_wildcard_projection_query_without_changing_results() {
    // Arrange
    with_fallback();
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
            .await
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
fn should_apply_vector_distance_offset_after_ordering() {
    // Arrange
    with_fallback();
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
            ).await;
        cassie
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

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(1.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_cosine_distance_for_vector_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("cosine_distance_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_cosine_distance_projection";
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
                Some("same".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("orthogonal".to_string()),
                serde_json::json!({"embedding": [0.0, 1.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, cosine_distance(embedding, '[1,0]') AS distance FROM sql_cosine_distance_projection ORDER BY id",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("orthogonal".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(1.0));
        assert_eq!(result.rows[1][0], Value::String("same".to_string()));
        assert_eq!(result.rows[1][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_dot_product_for_vector_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("dot_product_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_dot_product_projection";
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
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT dot_product(embedding, '[3,4]') AS score FROM sql_dot_product_projection",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(11.0)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_l2_distance_for_vector_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("l2_distance_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_l2_distance_projection";
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
                serde_json::json!({"embedding": [4.0, 6.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT vector_distance(embedding, '[1,2]') AS distance FROM sql_l2_distance_projection",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_pgvector_operator_distances() {
    // Arrange
    with_fallback();
    let path = data_dir("pgvector_operator_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_pgvector_operator_projection";
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
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT embedding <-> '[1,0]' AS l2, embedding <=> '[1,0]' AS cosine, embedding <#> '[1,0]' AS dot FROM sql_pgvector_operator_projection",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Float64(1.0),
                Value::Float64(0.0),
                Value::Float64(-2.0)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_fulltext_top_k_by_score_with_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_top_k_limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_top_k_limit";
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
            ).await;
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
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha alpha alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "beta gamma"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_top_k_limit WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_fulltext_offset_after_score_ordering() {
    // Arrange
    with_fallback();
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
            ).await;
        cassie
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

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_unordered_fulltext_query_with_matching_search_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_unordered_match");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_match";
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
            ).await;
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "first", "body": "alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "second", "body": "bravo"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "third", "body": "alpha alpha"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_match WHERE search(body, 'alpha') LIMIT 1 OFFSET 1",
                vec![],
            )

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[0][1], Value::String("third".to_string()));
        assert!(matches!(result.rows[0][2], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_search_function_as_boolean_match() {
    // Arrange
    with_fallback();
    let path = data_dir("search_boolean_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_search_boolean_projection";
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
            )
            .await;
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search(body, 'alpha') AS matches_alpha, search(body, 'gamma') AS matches_gamma FROM sql_search_boolean_projection",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Bool(true), Value::Bool(false)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_search_score_as_numeric_relevance() {
    // Arrange
    with_fallback();
    let path = data_dir("search_score_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_search_score_projection";
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
            )
            .await;
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha alpha beta"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "gamma delta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_search_score_projection ORDER BY id",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(score) if score > 0.0));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[1][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_unordered_fulltext_mismatched_search_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_unordered_mismatch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_mismatch";
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
            ).await;
        cassie
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
                serde_json::json!({"body": "bravo"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_mismatch WHERE search(body, 'bravo')",
                vec![],
            )

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_unordered_fulltext_additional_filters_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_unordered_extra_filter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_extra_filter";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
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
            ).await;
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha alpha", "status": "pending"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha", "status": "approved"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_extra_filter WHERE search(body, 'alpha') AND status = 'approved'",
                vec![],
            )

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_hybrid_top_k_by_score_with_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_top_k_limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_limit";
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
            ).await;
        cassie
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_top_k_limit ORDER BY score DESC LIMIT 1",
                vec![],
            )

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_hybrid_offset_after_score_ordering() {
    // Arrange
    with_fallback();
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
            ).await;
        cassie
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

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_generate_hybrid_candidates_from_text_matches() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_text_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_text_candidates";
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
            )
            .await;
        cassie
            .midge
            .put_document(
                collection,
                Some("text_match".to_string()),
                serde_json::json!({"body": "red", "embedding": [100.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("vector_only".to_string()),
                serde_json::json!({"body": "blue", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        let before = cassie.metrics().await;
        let before_candidates = before["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_text_candidates ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .await
            .unwrap();
        let after = cassie.metrics().await;

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("text_match".to_string()));
        assert_eq!(
            after["hybrid"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_hybrid_text_candidate_without_vector() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_missing_vector");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_missing_vector";
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
            )
            .await;
        cassie
            .midge
            .put_document(
                collection,
                Some("text_without_vector".to_string()),
                serde_json::json!({"body": "red"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("ignored_non_match".to_string()),
                serde_json::json!({"body": "blue"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_missing_vector ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .await;

        // Assert
        let error = result.expect_err("text candidate should require a vector");
        assert!(error.to_string().contains("vector_score expects vector"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_complex_fulltext_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_complex_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_complex_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
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
            ).await;
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha alpha alpha", "status": "pending"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha", "status": "approved"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_complex_fallback WHERE search(body, 'alpha') AND status = 'approved' ORDER BY score DESC LIMIT 1",
                vec![],
            )

            .await.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_snippet_without_highlighting_generated_markup() {
    // Arrange
    with_fallback();
    let path = data_dir("snippet_generated_markup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_snippet_generated_markup";
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
            )
            .await;
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'alpha mark') AS excerpt FROM sql_snippet_generated_markup",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("<mark>alpha</mark> beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_describe_select_projection_with_column_metadata() {
    // Arrange
    with_fallback();
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

        // Act
        let columns = cassie
            .describe_sql("SELECT id, title, score FROM sql_describe_metadata")
            .await
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
fn should_explain_select_query_plan() {
    // Arrange
    with_fallback();
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
            )
            .await;
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_select_plan WHERE title = 'alpha' ORDER BY title LIMIT 1",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "QUERY PLAN");
        assert_eq!(result.rows.len(), 1);
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("collection=sql_explain_select_plan"));
        assert!(plan.contains("operators=Scan>Filter>Sort>Project>Offset>Limit"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_predicate_pushdown_for_literal_equality_filter() {
    // Arrange
    with_fallback();
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_predicate_pushdown WHERE title = 'alpha'",
                vec![],
            )
            .await
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

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            ).await;

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

            .await.unwrap();

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

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            ).await;

        cassie
            .midge
            .put_document(collection, Some("d1".to_string()), serde_json::json!({"n": 1}))

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE counter(n) AS (SELECT n FROM integration_recursive_cte WHERE n = 1 UNION ALL SELECT n FROM counter WHERE n = 1) SELECT n FROM counter ORDER BY n",
            vec![],
            )

            .await.unwrap();

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
        let session = cassie.create_session("tester", None);
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
            .iter()
            .any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_schema_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("rename_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .await
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "ALTER SCHEMA reporting RENAME TO reporting_archive",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "ALTER SCHEMA");
        assert!(!cassie.catalog.namespace_exists("reporting").await);
        assert!(cassie.catalog.namespace_exists("reporting_archive").await);
        assert!(!cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting"));
        assert!(cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting_archive"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_drop_schema_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .await
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(&session, "DROP SCHEMA reporting", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "DROP SCHEMA");
        assert!(!cassie.catalog.namespace_exists("reporting").await);
        assert!(!cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting"));

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
        let session = cassie.create_session("tester", None);

        // Act
        let create = cassie
            .execute_sql(
                &session,
                "CREATE TABLE constraint_docs (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE, status TEXT DEFAULT 'pending', score INT CHECK (score >= 18))",
                vec![],
            )

            .await.unwrap();

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
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE hydrated_constraints (id INT, email TEXT NOT NULL UNIQUE, score INT CHECK (score >= 0))",
                vec![],
            )

            .await.unwrap();

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
fn should_reject_insert_when_primary_key_is_duplicate() {
    // Arrange
    with_fallback();
    let path = data_dir("primary_key_duplicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE primary_key_duplicate (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO primary_key_duplicate (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO primary_key_duplicate (id, title) VALUES (1, 'beta')",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'id'"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_primary_key_is_null() {
    // Arrange
    with_fallback();
    let path = data_dir("primary_key_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE primary_key_null (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO primary_key_null (id, title) VALUES (NULL, 'alpha')",
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
fn should_reject_insert_when_unique_value_is_duplicate() {
    // Arrange
    with_fallback();
    let path = data_dir("unique_insert_duplicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_insert_duplicate (email TEXT UNIQUE)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'email'"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_unique_value_conflicts() {
    // Arrange
    with_fallback();
    let path = data_dir("unique_update_conflict");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_update_conflict (email TEXT UNIQUE)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_update_conflict (email) VALUES ('a@example.com')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_update_conflict (email) VALUES ('b@example.com')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE unique_update_conflict SET email = 'a@example.com' WHERE email = 'b@example.com'",
                vec![],
            )
            .await;

        // Assert
        assert!(updated.is_err());
        assert!(updated
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'email'"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_unique_index_value_is_duplicate() {
    // Arrange
    with_fallback();
    let path = data_dir("unique_index_insert_duplicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_index_insert_duplicate (email TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX unique_index_email_idx ON unique_index_insert_duplicate USING btree (email)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unique index 'unique_index_email_idx' failed"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_unique_index_value_conflicts() {
    // Arrange
    with_fallback();
    let path = data_dir("unique_index_update_conflict");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_index_update_conflict (email TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX unique_index_update_email_idx ON unique_index_update_conflict USING btree (email)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_update_conflict (email) VALUES ('a@example.com')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_update_conflict (email) VALUES ('b@example.com')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE unique_index_update_conflict SET email = 'a@example.com' WHERE email = 'b@example.com'",
                vec![],
            )
            .await;

        // Assert
        assert!(updated.is_err());
        assert!(updated
            .unwrap_err()
            .to_string()
            .contains("unique index 'unique_index_update_email_idx' failed"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_check_constraint_fails() {
    // Arrange
    with_fallback();
    let path = data_dir("check_insert_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE check_insert_failure (score INT CHECK (score >= 18))",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO check_insert_failure (score) VALUES (17)",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("check constraint failed"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_check_constraint_fails() {
    // Arrange
    with_fallback();
    let path = data_dir("check_update_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE check_update_failure (score INT CHECK (score >= 18))",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO check_update_failure (score) VALUES (20)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE check_update_failure SET score = 17",
                vec![],
            )
            .await;

        // Assert
        assert!(updated.is_err());
        let message = updated.unwrap_err().to_string();
        assert!(
            message.contains("check constraint failed"),
            "expected check constraint error, got {message}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
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
        cassie.midge.create_namespace("analytics").unwrap();

        let initial = cassie.midge.list_namespaces();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA IF NOT EXISTS analytics", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "CREATE SCHEMA");
        let namespaced = cassie.midge.list_namespaces();
        assert_eq!(namespaced.len(), initial.len());
        assert!(namespaced.iter().any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_column_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("rename_column");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rename_column_docs (id TEXT, title TEXT)",
                vec![],
            )
            .await
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
        let result = cassie
            .execute_sql(
                &session,
                "ALTER TABLE rename_column_docs RENAME COLUMN title TO headline",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT id, headline FROM rename_column_docs ORDER BY id",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(result.command, "ALTER TABLE");
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(selected.rows[0][1], Value::String("alpha".to_string()));
        let schema = cassie
            .catalog
            .get_schema("rename_column_docs")
            .await
            .expect("schema should exist");
        assert!(schema.fields.iter().any(|field| field.name == "headline"));
        assert!(!schema.fields.iter().any(|field| field.name == "title"));

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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_returning (title TEXT, body TEXT)",
                vec![],
            )

            .await.unwrap();

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
        let session = cassie.create_session("tester", None);
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
fn should_insert_multiple_values_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_multiple_values");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_multiple_values (title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_multiple_values (title, score) VALUES ('alpha', 1), ('beta', 2) RETURNING title, score",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.command, "INSERT 0 2");
        assert_eq!(
            inserted.rows,
            vec![
                vec![Value::String("alpha".to_string()), Value::Int64(1)],
                vec![Value::String("beta".to_string()), Value::Int64(2)]
            ]
        );

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
        let session = cassie.create_session("tester", None);
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
fn should_execute_insert_returning_wildcard() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_returning_wildcard");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_returning_wildcard (title TEXT, body TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_wildcard (title, body) VALUES ('alpha', 'first') RETURNING *",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.columns[0].name, "_id");
        assert_eq!(inserted.columns[1].name, "title");
        assert_eq!(inserted.columns[2].name, "body");
        assert!(matches!(&inserted.rows[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(inserted.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(inserted.rows[0][2], Value::String("first".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_insert_returning_scalar_function() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_returning_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_returning_function (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_function (title) VALUES ('ALPHA') RETURNING lower(title) AS normalized",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.columns[0].name, "normalized");
        assert_eq!(
            inserted.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_returning_unknown_function() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_returning_unknown_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_returning_unknown_function (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_unknown_function (title) VALUES ('ALPHA') RETURNING missing_fn(title)",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unsupported function"));

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
        let session = cassie.create_session("tester", None);
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
fn should_reject_insert_values_when_not_null_column_is_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_missing_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_missing_not_null (title TEXT NOT NULL, body TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_missing_not_null (body) VALUES ('first')",
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )

            .await.unwrap();

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
fn should_preserve_explicit_insert_value_when_default_exists() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_explicit_default");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_explicit_default (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_explicit_default (id, status) VALUES (1, 'done') RETURNING status",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(inserted.rows.len(), 1);
        assert_eq!(inserted.rows[0][0], Value::String("done".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_query_rows_after_creating_secondary_index() {
    // Arrange
    with_fallback();
    let path = data_dir("secondary_index_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE secondary_index_query (email TEXT, title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX secondary_email_idx ON secondary_index_query USING btree (email)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('b@example.com', 'beta')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM secondary_index_query WHERE email = 'b@example.com'",
                vec![],
            )
            .await
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
fn should_query_rows_after_creating_composite_index() {
    // Arrange
    with_fallback();
    let path = data_dir("composite_index_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE composite_index_query (tenant_id TEXT, status TEXT, title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX composite_tenant_status_idx ON composite_index_query USING btree (tenant_id, status)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'open', 'alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'closed', 'beta')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-b', 'closed', 'gamma')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM composite_index_query WHERE tenant_id = 'tenant-a' AND status = 'closed'",
                vec![],
            )
            .await
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
fn should_round_trip_insert_values_vector_field() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_values_vector_round_trip");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_vector_round_trip (doc_id TEXT, embedding VECTOR(3))",
                vec![],
            )
            .await
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_vector_round_trip (doc_id, embedding) VALUES ('row-1', $1)",
                vec![Value::Vector(Vector::new(vec![1.0, 2.0, 3.0]))],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT embedding FROM insert_values_vector_round_trip WHERE doc_id = 'row-1'",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::Vector(Vector::new(vec![1.0, 2.0, 3.0]))]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_vector_index_when_embedding_dimensions_mismatch() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_index_embedding_dimension_mismatch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_index_embedding_dimension_mismatch (content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let created = cassie
            .execute_sql(
                &session,
                "CREATE INDEX vector_index_embedding_dimension_mismatch_idx ON vector_index_embedding_dimension_mismatch USING vector (embedding) WITH (source_field = content)",
                vec![],
            )
            .await;

        // Assert
        assert!(created.is_err());
        assert!(created
            .unwrap_err()
            .to_string()
            .contains("embedding dimension mismatch"));

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
        let session = cassie.create_session("tester", None);
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
fn should_reject_insert_values_with_duplicate_target_column() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_duplicate_column");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_duplicate_column (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_duplicate_column (title, title) VALUES ('alpha', 'beta')",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("duplicated"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_with_unknown_target_column() {
    // Arrange
    with_fallback();
    let path = data_dir("insert_unknown_column");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_unknown_column (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_unknown_column (missing) VALUES ('alpha')",
                vec![],
            )
            .await;

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("does not exist"));

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
        let session = cassie.create_session("tester", None);
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
            .unwrap();
        let legacy_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:insert_values_row_blob:")
            .unwrap();

        // Assert
        assert_eq!(row_entries.len(), 1);
        assert!(legacy_entries.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_missing_sparse_row_fields_as_null() {
    // Arrange
    with_fallback();
    let path = data_dir("sparse_row_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sparse_row_projection (title TEXT, body TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sparse_row_projection (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM sparse_row_projection",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(selected.rows[0][1], Value::Null);

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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_source (title TEXT, score INT)",
                vec![],
            )

            .await.unwrap();
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_shape_source (title TEXT, body TEXT)",
                vec![],
            )

            .await.unwrap();
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_default_source (source_id INT)",
                vec![],
            )

            .await.unwrap();
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_where_returning (title TEXT, status TEXT)",
                vec![],
            )

            .await.unwrap();
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
fn should_execute_update_returning_scalar_function() {
    // Arrange
    with_fallback();
    let path = data_dir("update_returning_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_returning_function (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_returning_function (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_returning_function SET title = 'BETA' RETURNING lower(title) AS normalized",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(updated.columns[0].name, "normalized");
        assert_eq!(updated.rows, vec![vec![Value::String("beta".to_string())]]);

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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_preserve_id (title TEXT, body TEXT)",
                vec![],
            )

            .await.unwrap();
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
        let session = cassie.create_session("tester", None);
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
fn should_report_zero_rows_for_update_without_matches() {
    // Arrange
    with_fallback();
    let path = data_dir("update_no_match");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_no_match (title TEXT, status TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_no_match (title, status) VALUES ('alpha', 'old')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_no_match SET status = 'done' WHERE title = 'missing' RETURNING title",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(updated.command, "UPDATE 0");
        assert!(updated.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_with_duplicate_assignment_target() {
    // Arrange
    with_fallback();
    let path = data_dir("update_duplicate_assignment");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_duplicate_assignment (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_duplicate_assignment SET title = 'alpha', title = 'beta'",
                vec![],
            )
            .await;

        // Assert
        assert!(updated.is_err());
        assert!(updated.unwrap_err().to_string().contains("duplicated"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_with_unknown_assignment_target() {
    // Arrange
    with_fallback();
    let path = data_dir("update_unknown_assignment");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE update_unknown_assignment (title TEXT)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_unknown_assignment SET missing = 'alpha'",
                vec![],
            )
            .await;

        // Assert
        assert!(updated.is_err());
        assert!(updated.unwrap_err().to_string().contains("does not exist"));

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
        let session = cassie.create_session("tester", None);
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
fn should_execute_delete_returning_scalar_function() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_returning_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE delete_returning_function (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_returning_function (title) VALUES ('ALPHA')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_returning_function RETURNING lower(title) AS normalized",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(deleted.columns[0].name, "normalized");
        assert_eq!(deleted.rows, vec![vec![Value::String("alpha".to_string())]]);

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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);

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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);

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
        let session = cassie.create_session("tester", None);

        // Act
        let statement = cassie
            .execute_sql(
                &session,
                "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
                vec![],
            )
            .await;

        // Assert
        assert!(statement.is_err());
        assert!(statement.unwrap_err().to_string().contains("unsupported"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rollback_to_savepoint_discard_later_writes() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_savepoint_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_savepoint_rollback (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_rollback (title) VALUES ('beta')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_savepoint_rollback ORDER BY title",
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
fn should_release_savepoint_prevent_later_rollback_to_it() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_savepoint_release");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .await
            .unwrap();

        // Act
        cassie
            .execute_sql(&session, "RELEASE SAVEPOINT sp", vec![])
            .await
            .unwrap();
        let rollback = cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .await;

        // Assert
        assert!(rollback.is_err());
        assert!(rollback.unwrap_err().to_string().contains("savepoint"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_savepoint_outside_transaction() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_savepoint_outside");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let savepoint = cassie.execute_sql(&session, "SAVEPOINT sp", vec![]).await;

        // Assert
        assert!(savepoint.is_err());
        assert!(savepoint
            .unwrap_err()
            .to_string()
            .contains("active transaction"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rollback_to_savepoint_recover_failed_transaction() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_savepoint_failed_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_savepoint_failed_recovery (title TEXT NOT NULL)",
                vec![],
            )
            .await
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .await
            .unwrap();
        let failed_insert = cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_failed_recovery (title) VALUES (NULL)",
                vec![],
            )
            .await;
        assert!(failed_insert.is_err());

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .await
            .unwrap();
        let status = session.transaction_status().await;
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_failed_recovery (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_savepoint_failed_recovery",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(status, "in_transaction");
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
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
        let session = cassie.create_session("tester", None);
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
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
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
        let session = cassie.create_session("tester", None);
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
            .unwrap();
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .await
            .unwrap();
        let after_commit = cassie
            .midge
            .get_document("transaction_storage_routing", &row_id)
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
fn should_project_rows_with_cast_expressions() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_cast_expressions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_cast_expressions (score INT, active BOOLEAN, flag TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_cast_expressions (score, active, flag) VALUES (10, true, 't')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT CAST(score AS TEXT) AS score_text, score::FLOAT AS score_float, CAST(active AS INT) AS active_int, CAST(flag AS BOOLEAN) AS flag_bool FROM projection_cast_expressions",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("invalid_cast_expression");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE invalid_cast_expression (label TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO invalid_cast_expression (label) VALUES ('not-a-number')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT CAST(label AS INT) FROM invalid_cast_expression",
                vec![],
            )
            .await;

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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_outer (title TEXT)", vec![])

            .await.unwrap();
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_outer (title TEXT)",
                vec![],
            )

            .await.unwrap();
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
fn should_filter_rows_with_not_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_docs (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_docs (title) VALUES ('keep')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_docs (title) VALUES ('skip')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_docs WHERE NOT title = 'skip'",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("keep".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_exists_outer (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_exists_inner (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_exists_outer WHERE NOT EXISTS (SELECT title FROM predicate_not_exists_inner)",
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE join_users (user_key INT, name TEXT)",
                vec![],
            )

            .await.unwrap();
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE left_users (user_key INT, name TEXT)",
                vec![],
            )

            .await.unwrap();
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
fn should_execute_right_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_right");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE right_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE right_orders (order_user_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO right_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT right_users.name, right_orders.total FROM right_users RIGHT JOIN right_orders ON right_users.user_key = right_orders.order_user_key",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::Null, Value::Int64(42)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_full_outer_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_full_outer");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE full_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE full_orders (order_user_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO full_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO full_orders (order_user_key, total) VALUES (2, 42)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT full_users.name, full_orders.total FROM full_users FULL OUTER JOIN full_orders ON full_users.user_key = full_orders.order_user_key ORDER BY full_users.name NULLS LAST",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("join_cross");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cross_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cross_orders (order_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_users (user_key, name) VALUES (2, 'grace')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_orders (order_key, total) VALUES (10, 42)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT cross_users.name, cross_orders.total FROM cross_users CROSS JOIN cross_orders ORDER BY cross_users.name",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("join_lateral");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE lateral_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE lateral_orders (order_user_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_users (user_key, name) VALUES (2, 'grace')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (1, 99)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (2, 7)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT lateral_users.name, recent.total FROM lateral_users JOIN LATERAL (SELECT total FROM lateral_orders WHERE order_user_key = lateral_users.user_key ORDER BY total DESC LIMIT 1) AS recent ON true ORDER BY lateral_users.name",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("join_cross_apply");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_orders (order_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO apply_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO apply_orders (order_key, total) VALUES (10, 42)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT apply_users.name, recent.total FROM apply_users CROSS APPLY (SELECT total FROM apply_orders) AS recent ORDER BY apply_users.name",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("join_outer_apply");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE outer_apply_users (user_key INT, name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_missing_orders (order_key INT, total INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO outer_apply_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT outer_apply_users.name, recent.total FROM outer_apply_users OUTER APPLY (SELECT total FROM apply_missing_orders) AS recent ORDER BY outer_apply_users.name",
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
        let session = cassie.create_session("tester", None);
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

#[test]
fn should_execute_grouped_count_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_count");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_count_docs (category TEXT)",
                vec![],
            )

            .await.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('b')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM aggregate_count_docs GROUP BY category ORDER BY category",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("aggregate_numeric");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_numeric_sales (amount INT)",
                vec![],
            )
            .await
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (5)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_numeric_sales",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("aggregate_nulls");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_null_sales (amount INT)",
                vec![],
            )
            .await
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_null_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (NULL)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_null_sales",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("window_row_number");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE window_scores (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'first', 10)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'second', 20)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('b', 'third', 30)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, title, row_number() OVER (PARTITION BY category ORDER BY score DESC) AS rank FROM window_scores ORDER BY category ASC, rank ASC",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("window_basic_values");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE window_values (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'alpha', 30)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'beta', 20)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'gamma', 20)",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS rnk, dense_rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS dense, lag(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS prev, lead(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS next, first_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS first, last_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS last FROM window_values ORDER BY rnk ASC, title ASC",
                vec![],
            )
            .await
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
                    Value::String("gamma".to_string())
                ],
                vec![
                    Value::String("beta".to_string()),
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
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
    with_fallback();
    let path = data_dir("aggregate_having");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_having_sales (category TEXT, amount INT)",
                vec![],
            )

            .await.unwrap();
        for sql in [
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 7)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 5)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('b', 3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, SUM(amount) AS total FROM aggregate_having_sales GROUP BY category HAVING SUM(amount) > 10",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("a".to_string()), Value::Int64(12)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_distinct_query() {
    // Arrange
    with_fallback();
    let path = data_dir("distinct_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE distinct_docs (category TEXT)",
                vec![],
            )
            .await
            .unwrap();
        for sql in [
            "INSERT INTO distinct_docs (category) VALUES ('b')",
            "INSERT INTO distinct_docs (category) VALUES ('a')",
            "INSERT INTO distinct_docs (category) VALUES ('a')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT category FROM distinct_docs ORDER BY category",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("union_all_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_all_left (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE union_all_right (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_left (title) VALUES ('beta')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_right (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_right (title) VALUES ('beta')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_all_left UNION ALL SELECT title FROM union_all_right",
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
                vec![Value::String("beta".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_union_query_with_deduplication() {
    // Arrange
    with_fallback();
    let path = data_dir("union_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_left (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_right (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_left (title) VALUES ('beta')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_right (title) VALUES ('alpha')",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_right (title) VALUES ('beta')",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_left UNION SELECT title FROM union_right",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("intersect_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE intersect_left (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE intersect_right (title TEXT)",
                vec![],
            )
            .await
            .unwrap();
        for sql in [
            "INSERT INTO intersect_left (title) VALUES ('alpha')",
            "INSERT INTO intersect_left (title) VALUES ('beta')",
            "INSERT INTO intersect_right (title) VALUES ('beta')",
            "INSERT INTO intersect_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM intersect_left INTERSECT SELECT title FROM intersect_right",
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
fn should_execute_except_query() {
    // Arrange
    with_fallback();
    let path = data_dir("except_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE except_left (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE except_right (title TEXT)", vec![])
            .await
            .unwrap();
        for sql in [
            "INSERT INTO except_left (title) VALUES ('alpha')",
            "INSERT INTO except_left (title) VALUES ('beta')",
            "INSERT INTO except_right (title) VALUES ('beta')",
            "INSERT INTO except_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM except_left EXCEPT SELECT title FROM except_right",
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
fn should_execute_distinct_on_query_with_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("distinct_on_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE distinct_on_docs (tenant_id TEXT, title TEXT, score INT)",
                vec![],
            )
            .await
            .unwrap();
        for sql in [
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'low', 1)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'high', 9)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('b', 'only', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT ON (tenant_id) tenant_id, title FROM distinct_on_docs ORDER BY tenant_id ASC, score DESC",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("union_global_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_left (title TEXT)", vec![])
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_right (title TEXT)", vec![])
            .await
            .unwrap();
        for sql in [
            "INSERT INTO union_order_left (title) VALUES ('beta')",
            "INSERT INTO union_order_right (title) VALUES ('alpha')",
            "INSERT INTO union_order_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_order_left UNION ALL SELECT title FROM union_order_right ORDER BY title LIMIT 1 OFFSET 1",
                vec![],
            )
            .await
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
    with_fallback();
    let path = data_dir("union_all_chained");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("tester", None);
        for table in ["union_chain_a", "union_chain_b", "union_chain_c"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {table} (title TEXT)"), vec![])
                .await
                .unwrap();
        }
        for sql in [
            "INSERT INTO union_chain_a (title) VALUES ('alpha')",
            "INSERT INTO union_chain_b (title) VALUES ('beta')",
            "INSERT INTO union_chain_c (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).await.unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_chain_a UNION ALL SELECT title FROM union_chain_b UNION ALL SELECT title FROM union_chain_c ORDER BY title DESC",
                vec![],
            )
            .await
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
