#[test]
fn should_reject_unauthorized_access_to_admin_query_routes() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("unauthorized");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cases = query_endpoint_cases(base_url.as_str());

        for (method, url, body) in cases {
            let request = client.request(method, url);
            let request = if let Some(body) = body {
                request.json(&body)
            } else {
                request
            };

            // Act
            let response = request.send().await.expect("query request");

            // Assert
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_reject_nonfinite_admin_query_result() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("nonfinite-query-json");
    let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_nonfinite (value FLOAT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO rest_nonfinite VALUES (1e308)",
            vec![],
        )
        .expect("insert finite value");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();

        // Act
        let nonfinite = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query/execute",
            "SELECT value * 10 AS result FROM rest_nonfinite",
        )
        .await;
        let nonfinite_status = nonfinite.status();
        let nonfinite_payload = nonfinite
            .json::<serde_json::Value>()
            .await
            .expect("non-finite error payload");
        // Assert
        assert_eq!(nonfinite_status, StatusCode::BAD_REQUEST);
        assert!(nonfinite_payload["error"]
            .as_str()
            .is_some_and(|message| message.contains("non-finite")));

        stop_rest_server(shutdown, server).await;
    });
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn should_map_admin_query_errors_to_semantic_http_statuses() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("query-errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        seed_query_catalog(&cassie);
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();

        // Act
        let malformed = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query/execute",
            "SELECT FROM",
        )
        .await;
        let missing = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query/execute",
            "SELECT title FROM missing_rest_admin_query_docs",
        )
        .await;
        let unsupported = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query/execute",
            "COPY rest_admin_query_docs TO STDOUT",
        )
        .await;
        let malformed_status = malformed.status();
        let missing_status = missing.status();
        let unsupported_status = unsupported.status();
        let malformed_payload = malformed
            .json::<serde_json::Value>()
            .await
            .expect("malformed");
        let missing_payload = missing.json::<serde_json::Value>().await.expect("missing");
        let unsupported_payload = unsupported
            .json::<serde_json::Value>()
            .await
            .expect("unsupported");

        // Assert
        assert_eq!(malformed_status, StatusCode::BAD_REQUEST);
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
        assert_eq!(unsupported_status, StatusCode::NOT_IMPLEMENTED);
        assert!(malformed_payload["error"]
            .as_str()
            .expect("malformed error")
            .contains("SELECT"));
        assert!(missing_payload["error"]
            .as_str()
            .expect("missing error")
            .contains("missing_rest_admin_query_docs"));
        assert!(unsupported_payload["error"]
            .as_str()
            .expect("unsupported error")
            .contains("unsupported feature"));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
