#[test]
fn should_return_gateway_timeout_for_admin_query_deadlines() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("deadline");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_timeout_ms = 1;
        let cassie =
            Cassie::new_with_data_dir_and_config(&data_dir, config).expect("cassie with config");
        cassie.startup().expect("startup");
        let collection = canonical_relation_name("postgres", "public", "rest_admin_timeout_docs");
        cassie
            .midge
            .create_collection(
                &collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .expect("create timeout collection");
        cassie.register_collection(
            &collection,
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("seed timeout document");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let sql = format!(
            "{}SELECT title FROM rest_admin_timeout_docs",
            " ".repeat(1_000_000)
        );

        // Act
        let response =
            post_admin_query(&client, &base_url, "/api/v1/admin/query/execute", &sql).await;
        let status = response.status();
        let payload = response
            .json::<serde_json::Value>()
            .await
            .expect("timeout payload");

        // Assert
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(payload["error"], "query timeout exceeded");

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_acknowledge_admin_query_cancellation_before_returning() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("operation-cancellation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.cte_recursion_depth = 1_000_000;
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;
        let operation_id = uuid::Uuid::new_v4();
        let query_client = client.clone();
        let query_url = format!("{base_url}/api/v1/admin/query-executions");
        let query_cookie = admin_cookie.clone();

        // Act
        let query = tokio::spawn(async move {
            query_client
                .post(query_url)
                .header("cookie", query_cookie)
                .json(&serde_json::json!({
                    "operation_id": operation_id,
                    "database": "postgres", "sql": "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000) SELECT MAX(n) FROM seq"
                }))
                .send()
                .await
                .expect("query response")
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        let cancellation = client
            .delete(format!(
                "{base_url}/api/v1/admin/query-operations/{operation_id}"
            ))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("cancellation response");
        let cancellation_status = cancellation.status();
        let cancellation_payload = cancellation
            .json::<serde_json::Value>()
            .await
            .expect("cancellation payload");
        let query_status = query.await.expect("query task").status();

        // Assert
        assert_eq!(cancellation_status, StatusCode::OK);
        assert_eq!(cancellation_payload["operation_id"], operation_id.to_string());
        assert_eq!(cancellation_payload["cancelled"], true);
        assert_eq!(query_status.as_u16(), 499);

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
