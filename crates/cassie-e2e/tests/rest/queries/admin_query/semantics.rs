#[test]
fn should_scope_each_admin_request_to_its_explicit_database() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    let admin = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
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
    let data_dir = data_dir("execute");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    let data_dir = data_dir("authenticated-workflow");
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
    let data_dir = data_dir("validate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    let data_dir = data_dir("explain");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
fn should_hide_ungranted_schema_items_from_restricted_rest_sessions() {
    // Arrange
    let data_dir = data_dir("restricted-schema-visibility");
    let reader_password = Uuid::new_v4().to_string();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie
            .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
            .expect("admin");
        cassie
            .execute_sql(
                &admin,
                &format!("CREATE ROLE rest_schema_reader LOGIN PASSWORD '{reader_password}'"),
                Vec::new(),
            )
            .expect("create restricted schema reader");
        for sql in [
            "CREATE TABLE rest_schema_visible (id INTEGER)",
            "CREATE TABLE rest_schema_secret (token TEXT)",
            "CREATE TABLE rest_graph_visible (id INTEGER PRIMARY KEY)",
            "CREATE TABLE rest_graph_secret (id INTEGER PRIMARY KEY)",
            "CREATE TABLE rest_graph_edge (id INTEGER PRIMARY KEY, src INTEGER, dst INTEGER)",
            "CREATE VIEW rest_schema_secret_view AS SELECT token FROM rest_schema_secret",
            "CREATE INDEX rest_schema_secret_idx ON rest_schema_secret (token)",
            "CREATE PROPERTY GRAPH rest_visibility_graph VERTEX TABLES \
             (rest_graph_visible AS visible KEY (id) LABEL visible_node PROPERTIES (id), \
              rest_graph_secret AS secret KEY (id) LABEL secret_node PROPERTIES (id)) \
             EDGE TABLES (rest_graph_edge AS link KEY (id) \
              SOURCE KEY (src) REFERENCES visible(id) \
              DESTINATION KEY (dst) REFERENCES visible(id) \
              LABEL link PROPERTIES (id))",
            "CREATE VIEW rest_graph_view AS \
             SELECT direct.id FROM GRAPH_TABLE(rest_visibility_graph \
             MATCH (secret IS secret_node) COLUMNS (secret.id)) AS graph_rows \
             JOIN rest_graph_visible direct ON direct.id = graph_rows.id",
            "GRANT CONNECT ON DATABASE postgres TO rest_schema_reader",
            "GRANT USAGE ON SCHEMA public TO rest_schema_reader",
            "GRANT SELECT ON TABLE public.rest_graph_visible TO rest_schema_reader",
            "GRANT SELECT ON TABLE public.rest_graph_edge TO rest_schema_reader",
        ] {
            cassie.execute_sql(&admin, sql, Vec::new()).expect("setup");
        }
        let secret_index = crate::support_sql::canonical_test_index(
            &cassie,
            "rest_schema_secret",
            "rest_schema_secret_idx",
        );
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let reader_cookie =
            login_cookie_as(&client, &base_url, "rest_schema_reader", &reader_password).await;

        // Act
        let response = client
            .get(format!(
                "{base_url}/api/v1/admin/query/schema?database=postgres"
            ))
            .header("cookie", &reader_cookie)
            .send()
            .await
            .expect("schema request");
        let payload = response.json::<serde_json::Value>().await.expect("json");

        // Assert
        assert!(!contains_item(
            section_items(&payload, "tables"),
            &canonical_relation_name("postgres", "public", "rest_schema_secret")
        ));
        assert!(!contains_item(
            section_items(&payload, "tables"),
            &canonical_relation_name("postgres", "public", "rest_schema_visible")
        ));
        assert!(contains_item(
            section_items(&payload, "tables"),
            &canonical_relation_name("postgres", "public", "rest_graph_visible")
        ));
        assert!(!contains_item(
            section_items(&payload, "views"),
            &canonical_relation_name("postgres", "public", "rest_graph_view")
        ));
        assert!(!contains_item(
            section_items(&payload, "views"),
            &canonical_relation_name("postgres", "public", "rest_schema_secret_view")
        ));
        assert!(!contains_item(
            section_items(&payload, "indexes"),
            &secret_index
        ));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

struct RestSchemaPrivilegeFixture {
    reader_password: String,
    connect_only_password: String,
    visible_index: String,
    secret_index: String,
}

fn setup_rest_schema_privileges(
    cassie: &Cassie,
    admin: &cassie::app::CassieSession,
) -> RestSchemaPrivilegeFixture {
    let reader_password = Uuid::new_v4().to_string();
    let connect_only_password = Uuid::new_v4().to_string();
    for sql in [
        format!("CREATE ROLE rest_acl_reader LOGIN PASSWORD '{reader_password}'"),
        format!("CREATE ROLE rest_acl_connect_only LOGIN PASSWORD '{connect_only_password}'"),
    ] {
        cassie
            .execute_sql(admin, &sql, Vec::new())
            .expect("create schema role");
    }
    for sql in [
        "CREATE TABLE rest_acl_visible (id INTEGER, title TEXT)",
        "CREATE TABLE rest_acl_secret (id INTEGER, token TEXT)",
        "CREATE INDEX rest_acl_visible_idx ON rest_acl_visible (title)",
        "CREATE INDEX rest_acl_secret_idx ON rest_acl_secret (token)",
        "CREATE VIEW rest_acl_visible_view AS SELECT title FROM rest_acl_visible",
        "CREATE VIEW rest_acl_secret_view AS SELECT token FROM rest_acl_secret",
        "GRANT CONNECT ON DATABASE postgres TO rest_acl_reader",
        "GRANT USAGE ON SCHEMA public TO rest_acl_reader",
        "GRANT SELECT ON TABLE public.rest_acl_visible TO rest_acl_reader",
        "GRANT CONNECT ON DATABASE postgres TO rest_acl_connect_only",
    ] {
        cassie
            .execute_sql(admin, sql, Vec::new())
            .expect("setup schema visibility");
    }
    RestSchemaPrivilegeFixture {
        reader_password,
        connect_only_password,
        visible_index: crate::support_sql::canonical_test_index(
            cassie,
            "rest_acl_visible",
            "rest_acl_visible_idx",
        ),
        secret_index: crate::support_sql::canonical_test_index(
            cassie,
            "rest_acl_secret",
            "rest_acl_secret_idx",
        ),
    }
}

async fn request_schema(client: &Client, base_url: &str, cookie: &str) -> serde_json::Value {
    let response = client
        .get(format!(
            "{base_url}/api/v1/admin/query/schema?database=postgres"
        ))
        .header("cookie", cookie)
        .send()
        .await
        .expect("schema request");
    assert_eq!(response.status(), StatusCode::OK);
    response.json().await.expect("schema json")
}

fn assert_selected_schema_visibility(
    restricted: &serde_json::Value,
    fixture: &RestSchemaPrivilegeFixture,
) {
    let visible_name = canonical_relation_name("postgres", "public", "rest_acl_visible");
    let visible_table = section_items(restricted, "tables")
        .iter()
        .find(|item| item["label"] == visible_name)
        .expect("selected table");
    let visible_columns = visible_table["columns"]
        .as_array()
        .expect("visible columns");
    assert_eq!(visible_columns.len(), 2);
    assert!(visible_columns.iter().any(|column| column["name"] == "id"));
    assert!(visible_columns
        .iter()
        .any(|column| column["name"] == "title"));
    assert!(!contains_item(
        section_items(restricted, "tables"),
        &canonical_relation_name("postgres", "public", "rest_acl_secret")
    ));
    assert!(contains_item(
        section_items(restricted, "indexes"),
        &fixture.visible_index
    ));
    assert!(!contains_item(
        section_items(restricted, "indexes"),
        &fixture.secret_index
    ));
    assert!(contains_item(
        section_items(restricted, "views"),
        &canonical_relation_name("postgres", "public", "rest_acl_visible_view")
    ));
    assert!(!contains_item(
        section_items(restricted, "views"),
        &canonical_relation_name("postgres", "public", "rest_acl_secret_view")
    ));
}

fn assert_connect_only_schema_visibility(connect_only: &serde_json::Value) {
    for section in ["tables", "views", "indexes"] {
        assert!(
            section_items(connect_only, section).is_empty(),
            "CONNECT-only role must not see user {section}"
        );
    }
    assert!(section_items(connect_only, "tables")
        .iter()
        .flat_map(|item| item["columns"].as_array().into_iter().flatten())
        .all(|column| column["name"] != "token"));
}

fn assert_root_schema_visibility(root: &serde_json::Value, fixture: &RestSchemaPrivilegeFixture) {
    assert!(contains_item(
        section_items(root, "tables"),
        &canonical_relation_name("postgres", "public", "rest_acl_secret")
    ));
    assert!(contains_item(
        section_items(root, "indexes"),
        &fixture.secret_index
    ));
    assert!(contains_item(
        section_items(root, "views"),
        &canonical_relation_name("postgres", "public", "rest_acl_secret_view")
    ));
}

#[test]
fn should_limit_rest_schema_metadata_to_relations_with_select_privilege() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie
            .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
            .expect("admin session");
        let fixture = setup_rest_schema_privileges(&cassie, &admin);
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let reader_cookie = login_cookie_as(
            &client,
            &base_url,
            "rest_acl_reader",
            &fixture.reader_password,
        )
        .await;
        let connect_only_cookie = login_cookie_as(
            &client,
            &base_url,
            "rest_acl_connect_only",
            &fixture.connect_only_password,
        )
        .await;
        let root_cookie = login_cookie_as(
            &client,
            &base_url,
            "root",
            crate::support_sql::test_root_password(),
        )
        .await;

        // Act
        let restricted = request_schema(&client, &base_url, &reader_cookie).await;
        let connect_only = request_schema(&client, &base_url, &connect_only_cookie).await;
        let root = request_schema(&client, &base_url, &root_cookie).await;

        // Assert
        assert_selected_schema_visibility(&restricted, &fixture);
        assert_connect_only_schema_visibility(&connect_only);
        assert_root_schema_visibility(&root, &fixture);

        stop_rest_server(shutdown, server).await;
    });
}

#[test]
fn should_return_admin_query_schema_sections_in_stable_order() {
    // Arrange
    let data_dir = data_dir("schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
            vec![
                "tables",
                "views",
                "indexes",
                "udfs",
                "procedures",
                "property_graphs"
            ]
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
    let data_dir = data_dir("restful-aliases");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
            vec![
                "tables",
                "views",
                "indexes",
                "udfs",
                "procedures",
                "property_graphs"
            ]
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
#[test]
fn should_resolve_mixed_case_collections_with_percent_decoded_quotes() {
    // Arrange
    let data_dir = data_dir("mixed-case-rest-collection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie
            .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
            .expect("admin session");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cities (id INT PRIMARY KEY, name TEXT)",
                Vec::new(),
            )
            .expect("create lowercase table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE \"Cities\" (id INT PRIMARY KEY, name TEXT)",
                Vec::new(),
            )
            .expect("create mixed-case table");
        let (base_url, shutdown, server) = spawn_rest_server(cassie.clone()).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url).await;

        // Act
        let lowercase = client
            .post(format!("{base_url}/api/v1/collections/cities/documents"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"id": 1, "name": "lower"}))
            .send()
            .await
            .expect("lowercase collection write");
        let mixed = client
            .post(format!(
                "{base_url}/api/v1/collections/%22Cities%22/documents"
            ))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"id": 2, "name": "mixed"}))
            .send()
            .await
            .expect("mixed-case collection write");
        let folded = client
            .post(format!("{base_url}/api/v1/collections/CITIES/documents"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"id": 3, "name": "folded"}))
            .send()
            .await
            .expect("folded collection write");
        let percent_folded = client
            .post(format!("{base_url}/api/v1/collections/%43ITIES/documents"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"id": 4, "name": "percent-folded"}))
            .send()
            .await
            .expect("percent-decoded folded collection write");

        // Assert
        assert_eq!(lowercase.status(), StatusCode::OK);
        assert_eq!(mixed.status(), StatusCode::OK);
        assert_eq!(folded.status(), StatusCode::OK);
        assert_eq!(percent_folded.status(), StatusCode::OK);
        let lower_rows = cassie
            .midge
            .scan_documents("postgres.public.cities")
            .expect("lowercase documents");
        let mixed_rows = cassie
            .midge
            .scan_documents("postgres.public.\"Cities\"")
            .expect("mixed-case documents");
        assert_eq!(lower_rows[0].payload["name"], "lower");
        assert_eq!(mixed_rows[0].payload["name"], "mixed");
        assert_eq!(lower_rows.len(), 3);
        let names = lower_rows
            .iter()
            .map(|row| row.payload["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&Some("lower")));
        assert!(names.contains(&Some("folded")));
        assert!(names.contains(&Some("percent-folded")));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
