// REST cancellation contract:
// - operation IDs remain duplicate IDs while active and for 1,024 completed entries;
// - an active duplicate is reported as 409 `"query operation ID is already active"`;
// - a completed duplicate/cancel is reported as 409 `"query operation already completed"`;
// - an unknown or owner-mismatched operation remains 404.
// - duplicate registration is rejected in active and completed states; completed entries
//   are retained for 1,024 operations, after which the oldest entry is evicted.

#[test]
fn should_return_gateway_timeout_for_admin_query_deadlines() {
    // Arrange
    let data_dir = data_dir("deadline");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = crate::support_sql::with_test_root_password(
            CassieRuntimeConfig::from_env().expect("runtime config"),
        );
        config.limits.query_timeout_ms = 1;
        let cassie =
            Cassie::new_in_memory_with_config(config).expect("cassie with config");
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = crate::support_sql::with_test_root_password(
            CassieRuntimeConfig::from_env().expect("runtime config"),
        );
        config.limits.cte_recursion_depth = 1_000_000;
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
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
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if cassie::rest::is_active_for_test(operation_id) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("query operation must register");
        let active_duplicate = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "operation_id": operation_id,
                "database": "postgres",
                "sql": "SELECT 2"
            }))
            .send()
            .await
            .expect("active duplicate query");
        let active_duplicate_status = active_duplicate.status();
        let active_duplicate_payload = active_duplicate
            .json::<serde_json::Value>()
            .await
            .expect("active duplicate payload");
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
        let repeated = client
            .delete(format!(
                "{base_url}/api/v1/admin/query-operations/{operation_id}"
            ))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("repeated cancellation response");
        let repeated_status = repeated.status();
        let repeated_payload = repeated
            .json::<serde_json::Value>()
            .await
            .expect("repeated cancellation payload");

        // Assert
        assert_eq!(cancellation_status, StatusCode::OK);
        assert_eq!(cancellation_payload["operation_id"], operation_id.to_string());
        assert_eq!(cancellation_payload["canceled"], true);
        assert_eq!(query_status.as_u16(), 499);
        assert_eq!(active_duplicate_status, StatusCode::CONFLICT);
        assert_eq!(
            active_duplicate_payload["error"],
            "query operation ID is already active"
        );
        assert_eq!(repeated_status, StatusCode::CONFLICT);
        assert_eq!(
            repeated_payload["error"],
            "query operation already completed"
        );

        stop_rest_server(shutdown, server).await;
    });
}

#[test]
fn should_return_conflicting_status_given_duplicate_completed_operation_id() {
    // Arrange
    let data_dir = data_dir("operation-completion-status");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;
        let operation_id = uuid::Uuid::new_v4();

        // Act
        let unknown = client
            .delete(format!(
                "{base_url}/api/v1/admin/query-operations/{}",
                uuid::Uuid::new_v4()
            ))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("unknown operation");
        let unknown_status = unknown.status();
        let completed = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "operation_id": operation_id,
                "database": "postgres",
                "sql": "SELECT 1"
            }))
            .send()
            .await
            .expect("completed query");
        let completed_status = completed.status();
        let duplicate = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "operation_id": operation_id,
                "database": "postgres",
                "sql": "SELECT 2"
            }))
            .send()
            .await
            .expect("duplicate query");
        let duplicate_status = duplicate.status();
        let canceled = client
            .delete(format!(
                "{base_url}/api/v1/admin/query-operations/{operation_id}"
            ))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("cancel completed operation");
        let canceled_status = canceled.status();
        let canceled_payload = canceled
            .json::<serde_json::Value>()
            .await
            .expect("cancel completed payload");

        // Assert
        assert_eq!(completed_status, StatusCode::OK);
        assert_eq!(unknown_status, StatusCode::NOT_FOUND);
        assert_eq!(duplicate_status, StatusCode::CONFLICT);
        assert_eq!(canceled_status, StatusCode::CONFLICT);
        assert_eq!(
            canceled_payload["error"],
            "query operation already completed"
        );

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
