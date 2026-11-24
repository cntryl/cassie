#[test]
fn should_report_diagnostic_plan_cache_hit_after_query_execution() {
    // Arrange
    let path = data_dir("diagnostic_cache_hit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
            &session.search_path(),
        );

        // Assert
        assert!(hit);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reuse_cached_plan_for_equivalent_sql_with_different_whitespace() {
    // Arrange
    let path = data_dir("reuse_equivalent_sql_whitespace");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(first.rows().len(), 1);
        assert_eq!(second.rows().len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reuse_cached_execution_result_without_additional_storage_reads() {
    // Arrange
    let path = data_dir("execution_result_cache_hit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(first.rows(), second.rows());
        assert_eq!(first.rows().len(), 1);
        assert!(middle_reads > before_reads);
        assert_eq!(after_reads, middle_reads);
        assert_eq!(after["query"]["count"].as_u64(), Some(2));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_isolate_cached_plans_by_database() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("alice", None);
    for database in ["primary_db", "analytics_db"] {
        cassie
            .execute_sql(&setup, &format!("CREATE DATABASE {database}"), vec![])
            .expect("create database");
    }
    let primary = cassie.create_session("alice", Some("primary_db".to_string()));
    let analytics = cassie.create_session("alice", Some("analytics_db".to_string()));
    for (session, title) in [(&primary, "alpha"), (&analytics, "beta")] {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE plan_cache_database_docs (title TEXT)",
                vec![],
            )
            .expect("create same-named database relation");
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO plan_cache_database_docs VALUES ('{title}')"),
                vec![],
            )
            .expect("seed distinguishing database row");
    }
    let before = cassie.metrics();
    let misses_before = before["plan_cache"]["misses"].as_u64().expect("misses");
    let hits_before = before["plan_cache"]["hits"].as_u64().expect("hits");
    let sql = "SELECT title FROM plan_cache_database_docs";

    // Act
    let first = cassie
        .execute_sql(&primary, sql, vec![])
        .expect("primary query");
    let second = cassie
        .execute_sql(&analytics, sql, vec![])
        .expect("analytics query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(
        first.rows(),
        vec![vec![cassie::types::Value::String("alpha".to_string())]]
    );
    assert_eq!(
        second.rows(),
        vec![vec![cassie::types::Value::String("beta".to_string())]]
    );
    assert_eq!(
        metrics["plan_cache"]["misses"].as_u64().expect("misses") - misses_before,
        2
    );
    assert_eq!(
        metrics["plan_cache"]["hits"].as_u64().expect("hits") - hits_before,
        0
    );
    drop(first);
    drop(second);
    drop(primary);
    drop(analytics);
    drop(setup);
    cassie.shutdown();
    drop(cassie);
}

#[test]
fn should_keep_first_non_durable_plan_miss_out_of_cf2() {
    // Arrange
    let path = data_dir("first_non_durable_miss_out_of_cf2");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(result.rows().len(), 1);
        assert_eq!(metrics["storage"]["temp"]["writes"].as_u64(), Some(0));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_separate_cached_plans_by_adaptive_config() {
    // Arrange
    use_local_storage();
    let path = data_dir("adaptive_config_key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection =
                canonical_relation_name("postgres", "public", "plan_cache_adaptive_config_docs");
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(&collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                &collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    &collection,
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
        assert_eq!(result.rows().len(), 1);
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_evict_oldest_plan_when_cache_capacity_is_one() {
    // Arrange
    let path = data_dir("eviction");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.plan_cache_entries = 1;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
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
        assert_eq!(first.rows().len(), 1);
        assert_eq!(second.rows().len(), 1);
        assert_eq!(third.rows().len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(3));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));
        assert_eq!(metrics["plan_cache"]["evictions"].as_u64(), Some(2));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_promote_non_durable_l1_plan_without_extra_cf2_reads_on_second_hit() {
    // Arrange
    let path = data_dir("l1_promotion_without_extra_cf2_reads");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(first.rows().len(), 1);
        assert_eq!(second.rows().len(), 1);
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
