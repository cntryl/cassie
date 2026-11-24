#[test]
fn should_reuse_cf2_cached_plan_after_restart_without_l1_state() {
    // Arrange
    use_local_storage();
    let path = data_dir("reuse_after_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection =
                canonical_relation_name("postgres", "public", "plan_cache_restart_docs");
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
                .ingest_document(&collection, serde_json::json!({"title": "alpha"}))
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

            assert_eq!(first.rows().len(), 1);
            assert_eq!(second.rows().len(), 1);
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
        assert_eq!(result.rows().len(), 1);
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}
