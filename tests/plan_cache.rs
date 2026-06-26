use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::ExecutionMode;
use cassie::sql::parse_statement;
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

fn adaptive_execution_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::default();
    config.limits.adaptive_execution_enabled = true;
    config.limits.adaptive_min_cost_savings_bps = 100;
    config
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
        cassie.catalog.register_collection(
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

        let session_one = cassie.create_session("alice", None);
        let session_two = cassie.create_session("bob", None);

        // Act
        let first = cassie
            .execute_sql(
                &session_one,
                "SELECT title FROM plan_cache_docs WHERE title = $1",
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session_two,
                "SELECT title FROM plan_cache_docs WHERE title = $1",
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let metrics = cassie.metrics();

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
fn should_report_diagnostic_plan_cache_hit_after_query_execution() {
    // Arrange
    with_fallback();
    let path = data_dir("diagnostic_cache_hit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_diagnostic_docs";
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
        cassie.catalog.register_collection(
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

        let session = cassie.create_session("alice", None);
        let sql = "SELECT title FROM plan_cache_diagnostic_docs WHERE title = $1";
        let params = vec![cassie::types::Value::String("alpha".to_string())];
        cassie.execute_sql(&session, sql, params.clone()).unwrap();
        let parsed = parse_statement(sql).unwrap();

        // Act
        let hit = cassie.plan_cache_hit_for_diagnostics(
            &parsed,
            &params,
            ExecutionMode::SimpleQuery,
            session.database.clone(),
        );

        // Assert
        assert!(hit);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reuse_cached_plan_for_equivalent_sql_with_different_whitespace() {
    // Arrange
    with_fallback();
    let path = data_dir("reuse_equivalent_sql_whitespace");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_whitespace_docs";
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
        cassie.catalog.register_collection(
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

        let session = cassie.create_session("alice", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_whitespace_docs WHERE title = $1",
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "  select   title  from   plan_cache_whitespace_docs where   title = $1  ",
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reuse_cached_execution_result_without_additional_storage_reads() {
    // Arrange
    with_fallback();
    let path = data_dir("execution_result_cache_hit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "execution_cache_docs";
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
        cassie.catalog.register_collection(
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

        let session = cassie.create_session("alice", None);
        let before = cassie.metrics();
        let before_reads = before["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM execution_cache_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let middle = cassie.metrics();
        let middle_reads = middle["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM execution_cache_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();
        let after_reads = after["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        // Assert
        assert_eq!(first.rows, second.rows);
        assert_eq!(first.rows.len(), 1);
        assert!(middle_reads > before_reads);
        assert_eq!(after_reads, middle_reads);
        assert_eq!(after["query"]["count"].as_u64(), Some(2));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_invalidate_cached_execution_result_after_write() {
    // Arrange
    with_fallback();
    let path = data_dir("execution_result_cache_invalidation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "execution_cache_invalidation_docs";
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
        cassie.catalog.register_collection(
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

        let session = cassie.create_session("alice", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM execution_cache_invalidation_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO execution_cache_invalidation_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM execution_cache_invalidation_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 2);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_isolate_cached_plans_by_database() {
    // Arrange
    with_fallback();
    let path = data_dir("database_isolation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_database_docs";
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
        cassie.catalog.register_collection(
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

        let primary = cassie.create_session("alice", Some("primary_db".to_string()));
        let analytics = cassie.create_session("alice", Some("analytics_db".to_string()));
        let sql = "SELECT title FROM plan_cache_database_docs WHERE title = 'alpha'";

        // Act
        let first = cassie.execute_sql(&primary, sql, vec![]).unwrap();
        let second = cassie.execute_sql(&analytics, sql, vec![]).unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(2));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_first_non_durable_plan_miss_out_of_cf2() {
    // Arrange
    with_fallback();
    let path = data_dir("first_non_durable_miss_out_of_cf2");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_first_miss_docs";
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
        cassie.catalog.register_collection(
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
        let session = cassie.create_session("alice", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_first_miss_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["storage"]["temp"]["writes"].as_u64(), Some(0));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

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
            cassie.startup().unwrap();

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
            cassie.catalog.register_collection(
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

            let session = cassie.create_session("alice", None);

            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();

            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            cassie.shutdown();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("alice", None);

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = restarted.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_separate_cached_plans_by_adaptive_config() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_config_key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "plan_cache_adaptive_config_docs";
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
            cassie.catalog.register_collection(
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

            let session = cassie.create_session("alice", None);
            let sql = "SELECT title FROM plan_cache_adaptive_config_docs WHERE title = 'alpha'";
            cassie.execute_sql(&session, sql, vec![]).unwrap();
            cassie.execute_sql(&session, sql, vec![]).unwrap();
            cassie.shutdown();
        }

        let restarted =
            Cassie::new_with_data_dir_and_config(&path, adaptive_execution_config()).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("alice", None);

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_adaptive_config_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = restarted.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));

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
        cassie.catalog.register_collection(
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

        let session = cassie.create_session("alice", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_ddl_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE plan_cache_guard (id INT)", vec![])
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_ddl_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(2));
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
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
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
        cassie.catalog.register_collection(
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

        let session = cassie.create_session("alice", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_eviction_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_eviction_docs WHERE title = 'beta'",
                vec![],
            )
            .unwrap();
        let third = cassie
            .execute_sql(
                &session,
                "SELECT title FROM plan_cache_eviction_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

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
        cassie.startup().unwrap();
        let session = cassie.create_session("alice", None);
        let before = cassie.metrics();

        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let after = cassie.metrics();

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

#[test]
fn should_promote_non_durable_l1_plan_without_extra_cf2_reads_on_second_hit() {
    // Arrange
    with_fallback();
    let path = data_dir("l1_promotion_without_extra_cf2_reads");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "plan_cache_l1_promotion_docs";
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
        cassie.catalog.register_collection(
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
        let session = cassie.create_session("alice", None);

        let first_sql = "SELECT title FROM plan_cache_l1_promotion_docs WHERE title = 'alpha'";
        // Act
        let first = cassie.execute_sql(&session, first_sql, vec![]).unwrap();
        let after_first = cassie.metrics();
        let second = cassie.execute_sql(&session, first_sql, vec![]).unwrap();
        let after_second = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(
            after_second["storage"]["temp"]["reads"].as_u64(),
            after_first["storage"]["temp"]["reads"].as_u64()
        );
        let writes_after_first = after_first["storage"]["temp"]["writes"]
            .as_u64()
            .expect("first temp writes");
        let writes_after_second = after_second["storage"]["temp"]["writes"]
            .as_u64()
            .expect("second temp writes");
        assert_eq!(writes_after_second.saturating_sub(writes_after_first), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}
