#[test]
fn should_reuse_cached_plan_across_sessions_without_sharing_bind_values() {
    // Arrange
    use_local_storage();
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
        assert_eq!(first.rows().len(), 1);
        assert_eq!(second.rows().len(), 1);
        assert_eq!(
            first.rows()[0][0],
            cassie::types::Value::String("alpha".to_string())
        );
        assert_eq!(
            second.rows()[0][0],
            cassie::types::Value::String("beta".to_string())
        );
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}
