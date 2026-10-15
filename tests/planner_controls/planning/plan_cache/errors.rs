#[test]
fn should_invalidate_cached_execution_result_after_write() {
    // Arrange
    use_local_storage();
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
        assert_eq!(first.rows().len(), 1);
        assert_eq!(second.rows().len(), 2);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_invalidate_cached_plan_after_ddl_changes_catalog_state() {
    // Arrange
    use_local_storage();
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
        assert_eq!(first.rows().len(), 1);
        assert_eq!(second.rows().len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(2));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}
