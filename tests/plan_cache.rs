use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-plan-cache-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

#[test]
fn should_reuse_cached_plan_across_sessions_without_sharing_bind_values() {
    // Arrange
    with_fallback();
    let path = data_dir("reuse_across_sessions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_docs";
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
            .catalog
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

        let session_one = cassie.create_session("alice", None);
        let session_two = cassie.create_session("bob", None);

        // Act
        let first = cassie
            .execute_sql(
                &session_one,
                "SELECT title FROM plan_cache_docs WHERE title = $1",
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            
            .await.unwrap();
        let second = cassie
            .execute_sql(
                &session_two,
                "SELECT title FROM plan_cache_docs WHERE title = $1",
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .await
            .unwrap();
        let metrics = cassie.metrics().await;

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(
            first.rows[0][0],
            cassie::types::Value::String("alpha".to_string())
        );
        assert_eq!(
            second.rows[0][0],
            cassie::types::Value::String("beta".to_string())
        );
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reuse_cf2_cached_plan_after_restart_without_l1_state() {
    // Arrange
    with_fallback();
    let path = data_dir("reuse_after_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().await.unwrap();

            let collection = "plan_cache_restart_docs";
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
                .catalog
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
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                
                .unwrap();

            let session = cassie.create_session("alice", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                    vec![],
                )
                
                .await.unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                    vec![],
                )
                .await
                .unwrap();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            cassie.shutdown().await;
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().await.unwrap();
        let session = restarted.create_session("alice", None);

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                vec![],
            )
            
            .await.unwrap();
        let metrics = restarted.metrics().await;

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_invalidate_cached_plan_after_ddl_changes_catalog_state() {
    // Arrange
    with_fallback();
    let path = data_dir("invalidate_after_ddl");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_ddl_docs";
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
            .catalog
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
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            
            .unwrap();

        let session = cassie.create_session("alice", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_ddl_docs WHERE title = 'alpha'",
                vec![],
            )
            
            .await.unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE plan_cache_guard (id INT)", vec![])
            .await
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_ddl_docs WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();
        let metrics = cassie.metrics().await;

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(3));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_evict_oldest_plan_when_cache_capacity_is_one() {
    // Arrange
    with_fallback();
    let path = data_dir("eviction");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env();
        config.limits.plan_cache_entries = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "plan_cache_eviction_docs";
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
            .catalog
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

        let session = cassie.create_session("alice", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_eviction_docs WHERE title = 'alpha'",
                vec![],
            )
            
            .await.unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_eviction_docs WHERE title = 'beta'",
                vec![],
            )
            .await
            .unwrap();
        let third = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_eviction_docs WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();
        let metrics = cassie.metrics().await;

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(third.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(3));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));
        assert_eq!(metrics["plan_cache"]["evictions"].as_u64(), Some(2));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_cache_transaction_control_statements() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_controls_bypass_cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let session = cassie.create_session("alice", None);
        let before = cassie.metrics().await;

        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).await.unwrap();
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .await
            .unwrap();
        let after = cassie.metrics().await;

        // Assert
        assert_eq!(
            after["plan_cache"]["hits"].as_u64(),
            before["plan_cache"]["hits"].as_u64()
        );
        assert_eq!(
            after["plan_cache"]["misses"].as_u64(),
            before["plan_cache"]["misses"].as_u64()
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
