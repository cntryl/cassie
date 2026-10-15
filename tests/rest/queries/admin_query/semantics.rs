#[test]
fn should_scope_each_admin_request_to_its_explicit_database() {
    // Arrange
    use_local_storage();
    let cassie = Cassie::new_with_data_dir(data_dir("explicit-database-scope")).expect("cassie");
    let admin = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin session");
    cassie
        .execute_sql(&admin, "CREATE DATABASE analytics", Vec::new())
        .expect("create analytics database");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url).await;

        // Act
        let databases = client
            .get(format!("{base_url}/api/v1/admin/databases"))
            .header("cookie", &cookie)
            .send()
            .await
            .expect("database discovery");
        let postgres = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"database": "postgres", "sql": "SELECT current_database()"}))
            .send()
            .await
            .expect("postgres query");
        let analytics = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"database": "analytics", "sql": "SELECT current_database()"}))
            .send()
            .await
            .expect("analytics query");
        let missing = client
            .post(format!("{base_url}/api/v1/admin/query-validations"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"sql": "SELECT 1"}))
            .send()
            .await
            .expect("missing database");
        let unknown = client
            .get(format!("{base_url}/api/v1/admin/catalog?database=missing"))
            .header("cookie", &cookie)
            .send()
            .await
            .expect("unknown database");

        // Assert
        assert_eq!(databases.status(), StatusCode::OK);
        let discovered = databases
            .json::<serde_json::Value>()
            .await
            .expect("database json");
        assert_eq!(discovered[0]["name"], "analytics");
        assert_eq!(discovered[1]["name"], "postgres");
        assert_eq!(
            postgres
                .json::<serde_json::Value>()
                .await
                .expect("postgres json")["rows"][0][0],
            "postgres"
        );
        assert_eq!(
            analytics
                .json::<serde_json::Value>()
                .await
                .expect("analytics json")["rows"][0][0],
            "analytics"
        );
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        stop_rest_server(shutdown, server).await;
    });
}

#[test]
fn should_execute_admin_query_through_rest() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("execute");
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
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/admin/query/execute"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "database": "postgres", "sql": "SELECT q.title FROM (SELECT title FROM rest_admin_query_docs) AS q ORDER BY q.title"
            }))
            .send()
            .await
            .expect("execute request");
        let status = response.status();
        let payload = response.json::<serde_json::Value>().await.expect("json");

        // Assert
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["command"], "SELECT");
        assert_eq!(payload["columns"][0]["name"], "title");
        assert_eq!(payload["rows"].as_array().expect("rows").len(), 1);
        assert_eq!(payload["rows"][0][0], "alpha");

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_complete_admin_query_workflow_given_one_authenticated_session() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("authenticated-workflow");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let mut responses = Vec::new();
        for sql in [
            "CREATE TABLE ui_demo (demo_id INT PRIMARY KEY, name TEXT NOT NULL)",
            "INSERT INTO ui_demo (demo_id, name) VALUES (1, 'Ada'), (2, 'Grace')",
            "SELECT demo_id, name FROM ui_demo ORDER BY demo_id",
        ] {
            let response = client
                .post(format!("{base_url}/api/v1/admin/query-executions"))
                .header("cookie", &admin_cookie)
                .json(&serde_json::json!({ "database": "postgres", "sql": sql }))
                .send()
                .await
                .expect("query execution");
            responses.push((
                response.status(),
                response.json::<serde_json::Value>().await.expect("json"),
            ));
        }

        // Assert
        assert_eq!(responses[0].0, StatusCode::OK);
        assert_eq!(responses[0].1["command"], "CREATE TABLE");
        assert_eq!(responses[1].0, StatusCode::OK);
        assert_eq!(responses[1].1["command"], "INSERT 0 2");
        assert_eq!(responses[2].0, StatusCode::OK);
        assert_eq!(responses[2].1["command"], "SELECT");
        assert_eq!(
            responses[2].1["rows"],
            serde_json::json!([[1, "Ada"], [2, "Grace"]])
        );

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_validate_admin_query_through_rest() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("validate");
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
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let valid = client
            .post(format!("{base_url}/api/v1/admin/query/validate"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "database": "postgres", "sql": "SELECT title FROM rest_admin_query_docs"
            }))
            .send()
            .await
            .expect("valid request");
        let valid_status = valid.status();
        let valid_payload = valid.json::<serde_json::Value>().await.expect("valid json");
        let malformed = client
            .post(format!("{base_url}/api/v1/admin/query/validate"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({"database": "postgres", "sql": "SELECT FROM"}))
            .send()
            .await
            .expect("malformed request");
        let malformed_status = malformed.status();
        let malformed_payload = malformed
            .json::<serde_json::Value>()
            .await
            .expect("malformed json");

        // Assert
        assert_eq!(valid_status, StatusCode::OK);
        assert_eq!(valid_payload["valid"].as_bool(), Some(true));
        assert_eq!(valid_payload["command"], "SELECT");
        assert_eq!(valid_payload["columns"][0]["name"], "title");
        assert_eq!(malformed_status, StatusCode::BAD_REQUEST);
        assert!(malformed_payload["error"].as_str().is_some());

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_explain_admin_query_through_rest() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("explain");
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
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/admin/query/explain"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "database": "postgres", "sql": "SELECT title FROM rest_admin_query_docs WHERE title = 'alpha'"
            }))
            .send()
            .await
            .expect("explain request");
        let status = response.status();
        let payload = response.json::<serde_json::Value>().await.expect("json");

        // Assert
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["command"], "EXPLAIN");
        assert_eq!(payload["columns"][0]["name"], "QUERY PLAN");
        assert_eq!(payload["plan"]["format_version"], 1);
        assert!(payload["plan"]["summary"]["collection"]
            .as_str()
            .expect("plan collection")
            .ends_with("rest_admin_query_docs"));
        assert_eq!(payload["plan"]["summary"]["access_path"], "index_seek");
        assert_eq!(
            payload["plan"]["summary"]["selected_index"],
            canonical_relation_name("postgres", "public", "rest_admin_query_title_idx")
        );
        assert_eq!(payload["plan"]["nodes"][0]["kind"], "read");
        assert_eq!(payload["plan"]["nodes"][0]["status"], "optimized");
        assert!(plan_feature_enabled(&payload["plan"], "predicate_pushdown"));
        assert!(plan_feature_enabled(&payload["plan"], "covered_index"));
        assert_eq!(
            payload["plan"]["diagnostics"]["access_path_reason"],
            "scalar-index-seek"
        );
        assert!(
            payload["plan"]["estimates"]["selected_cost"]
                .as_u64()
                .expect("selected cost")
                > 0
        );
        assert!(
            !payload["rows"].as_array().expect("rows").is_empty(),
            "explain should return at least one plan row"
        );

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_return_admin_query_schema_sections_in_stable_order() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("schema");
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
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .get(format!(
                "{base_url}/api/v1/admin/query/schema?database=postgres"
            ))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("schema request");
        let status = response.status();
        let payload = response.json::<serde_json::Value>().await.expect("json");
        let section_ids = payload["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .map(|section| section["id"].as_str().expect("section id"))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            section_ids,
            vec!["tables", "views", "indexes", "udfs", "procedures"]
        );
        assert_eq!(status, StatusCode::OK);
        assert!(contains_item(
            section_items(&payload, "tables"),
            &canonical_relation_name("postgres", "public", "rest_admin_query_docs")
        ));
        let table = section_items(&payload, "tables")
            .iter()
            .find(|item| item["name"] == "rest_admin_query_docs")
            .expect("table item");
        assert_eq!(table["database"], "postgres");
        assert_eq!(table["schema"], "public");
        assert_eq!(table["label"], "postgres.public.rest_admin_query_docs");
        assert_eq!(
            table["columns"][0]["id"],
            "column:postgres.public.rest_admin_query_docs:id"
        );
        assert_eq!(table["columns"][0]["data_type"], "int");
        assert_eq!(table["columns"][0]["primary_key"], true);
        assert_eq!(table["columns"][1]["name"], "title");
        assert_eq!(table["columns"][1]["primary_key"], false);
        assert!(contains_item(
            section_items(&payload, "views"),
            &canonical_relation_name("postgres", "public", "rest_admin_query_ready")
        ));
        assert!(contains_item(
            section_items(&payload, "indexes"),
            &canonical_relation_name("postgres", "public", "rest_admin_query_title_idx")
        ));
        assert!(contains_item(
            section_items(&payload, "udfs"),
            &canonical_relation_name("postgres", "public", "rest_query_identity")
        ));
        assert!(contains_item(
            section_items(&payload, "procedures"),
            &canonical_relation_name("postgres", "public", "rest_query_store")
        ));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_serve_restful_admin_aliases() {
    // Arrange
    use_local_storage();
    let data_dir = data_dir("restful-aliases");
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
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let catalog_response = client
            .get(format!("{base_url}/api/v1/admin/catalog?database=postgres"))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("catalog request");
        let catalog_status = catalog_response.status();
        let catalog_payload = catalog_response
            .json::<serde_json::Value>()
            .await
            .expect("catalog json");
        let catalog_section_ids = catalog_payload["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .map(|section| section["id"].as_str().expect("section id"))
            .collect::<Vec<_>>();

        let execution_response = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query-executions",
            "SELECT title FROM rest_admin_query_docs ORDER BY title",
        )
        .await;
        let execution_status = execution_response.status();
        let execution_payload = execution_response
            .json::<serde_json::Value>()
            .await
            .expect("execution json");

        let validation_response = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query-validations",
            "SELECT title FROM rest_admin_query_docs ORDER BY title",
        )
        .await;
        let validation_status = validation_response.status();
        let validation_payload = validation_response
            .json::<serde_json::Value>()
            .await
            .expect("validation json");

        let explain_response = post_admin_query(
            &client,
            &base_url,
            "/api/v1/admin/query-explanations",
            "SELECT title FROM rest_admin_query_docs WHERE title = 'alpha'",
        )
        .await;
        let explain_status = explain_response.status();
        let explain_payload = explain_response
            .json::<serde_json::Value>()
            .await
            .expect("explain json");

        // Assert
        assert_eq!(
            catalog_section_ids,
            vec!["tables", "views", "indexes", "udfs", "procedures"]
        );
        assert_eq!(catalog_status, StatusCode::OK);
        assert!(contains_item(
            section_items(&catalog_payload, "tables"),
            &canonical_relation_name("postgres", "public", "rest_admin_query_docs")
        ));
        assert_eq!(execution_status, StatusCode::OK);
        assert_eq!(execution_payload["command"], "SELECT");
        assert_eq!(execution_payload["rows"][0][0], "alpha");
        assert_eq!(validation_status, StatusCode::OK);
        assert_eq!(validation_payload["valid"], true);
        assert_eq!(validation_payload["command"], "SELECT");
        assert_eq!(explain_status, StatusCode::OK);
        assert_eq!(explain_payload["command"], "EXPLAIN");
        assert_eq!(explain_payload["columns"][0]["name"], "QUERY PLAN");
        assert_eq!(explain_payload["plan"]["format_version"], 1);
        assert_eq!(explain_payload["plan"]["nodes"][0]["kind"], "read");
        assert_eq!(
            explain_payload["plan"]["diagnostics"]["access_path_reason"],
            "scalar-index-seek"
        );

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
