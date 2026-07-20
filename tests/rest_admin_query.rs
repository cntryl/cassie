use std::path::PathBuf;
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

type QueryEndpointCase = (reqwest::Method, String, Option<serde_json::Value>);

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cassie-rest-admin-query-{label}-{}",
        Uuid::new_v4()
    ))
}

async fn spawn_rest_server(
    cassie: Cassie,
) -> (
    String,
    Arc<Notify>,
    tokio::task::JoinHandle<Result<(), CassieError>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    drop(listener);

    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
        addr.to_string(),
        cassie,
        shutdown.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(75)).await;

    (format!("http://{addr}"), shutdown, server)
}

async fn stop_rest_server(
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
) {
    shutdown.notify_waiters();
    let _ = server.await;
}

fn seed_query_catalog(cassie: &Cassie) {
    let session = cassie
        .authenticate_role("postgres", Some("postgres"), None)
        .expect("admin session");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_admin_query_docs (id INT, title TEXT)",
            Vec::new(),
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO rest_admin_query_docs (id, title) VALUES (1, 'alpha')",
            Vec::new(),
        )
        .expect("insert document");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX rest_admin_query_title_idx ON rest_admin_query_docs USING btree (title)",
            Vec::new(),
        )
        .expect("create index");
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW rest_admin_query_ready AS SELECT title FROM rest_admin_query_docs",
            Vec::new(),
        )
        .expect("create view");
    cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION rest_query_identity(x INT) RETURNS INT AS \"x\"",
            Vec::new(),
        )
        .expect("create function");
    cassie
        .execute_sql(
            &session,
            r#"CREATE PROCEDURE rest_query_store(title TEXT) AS "INSERT INTO rest_admin_query_docs (id, title) VALUES (2, $1)""#,
            Vec::new(),
        )
        .expect("create procedure");
}

fn query_endpoint_cases(base_url: &str) -> Vec<QueryEndpointCase> {
    vec![
        (
            reqwest::Method::GET,
            format!("{base_url}/api/v1/admin/query/schema"),
            None,
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query/execute"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query/validate"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query/explain"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::GET,
            format!("{base_url}/api/v1/admin/catalog"),
            None,
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query-executions"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query-validations"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query-explanations"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
    ]
}

fn section_items<'a>(schema: &'a serde_json::Value, section_id: &str) -> &'a [serde_json::Value] {
    schema["sections"]
        .as_array()
        .expect("schema sections")
        .iter()
        .find(|section| section["id"] == section_id)
        .and_then(|section| section["items"].as_array())
        .map(Vec::as_slice)
        .expect("section items")
}

fn contains_item(items: &[serde_json::Value], label: &str) -> bool {
    items.iter().any(|item| item["label"] == label)
}

fn plan_feature_enabled(plan: &serde_json::Value, feature_id: &str) -> bool {
    plan["features"]
        .as_array()
        .expect("plan features")
        .iter()
        .any(|feature| feature["id"] == feature_id && feature["enabled"] == true)
}

async fn login_cookie(client: &Client, base_url: &str) -> String {
    client
        .post(format!("{base_url}/api/v1/auth/login"))
        .json(&serde_json::json!({
            "username": "postgres",
            "password": "postgres"
        }))
        .send()
        .await
        .expect("login request")
        .headers()
        .get("set-cookie")
        .expect("session cookie")
        .to_str()
        .expect("session cookie value")
        .split(';')
        .next()
        .expect("session cookie pair")
        .to_string()
}

async fn post_admin_query(
    client: &Client,
    base_url: &str,
    path: &str,
    sql: &str,
) -> reqwest::Response {
    let session_cookie = login_cookie(client, base_url).await;
    client
        .post(format!("{base_url}{path}"))
        .header("cookie", session_cookie)
        .json(&serde_json::json!({ "sql": sql }))
        .send()
        .await
        .expect("admin query request")
}

#[test]
fn should_reject_unauthorized_access_to_admin_query_routes() {
    // Arrange
    with_fallback();
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
fn should_execute_admin_query_through_rest() {
    // Arrange
    with_fallback();
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
                "sql": "SELECT title FROM rest_admin_query_docs ORDER BY title"
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

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_complete_admin_query_workflow_given_one_authenticated_session() {
    // Arrange
    with_fallback();
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
                .json(&serde_json::json!({ "sql": sql }))
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
    with_fallback();
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
                "sql": "SELECT title FROM rest_admin_query_docs"
            }))
            .send()
            .await
            .expect("valid request");
        let valid_status = valid.status();
        let valid_payload = valid.json::<serde_json::Value>().await.expect("valid json");
        let malformed = client
            .post(format!("{base_url}/api/v1/admin/query/validate"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({"sql": "SELECT FROM"}))
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
fn should_map_admin_query_errors_to_semantic_http_statuses() {
    // Arrange
    with_fallback();
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

#[test]
fn should_return_gateway_timeout_for_admin_query_deadlines() {
    // Arrange
    with_fallback();
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
fn should_explain_admin_query_through_rest() {
    // Arrange
    with_fallback();
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
                "sql": "SELECT title FROM rest_admin_query_docs WHERE title = 'alpha'"
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
    with_fallback();
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
            .get(format!("{base_url}/api/v1/admin/query/schema"))
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
    with_fallback();
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
            .get(format!("{base_url}/api/v1/admin/catalog"))
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

#[test]
fn should_return_method_not_allowed_for_known_admin_query_paths() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("method");
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

        for path in [
            "/api/v1/admin/query/execute",
            "/api/v1/admin/query-executions",
        ] {
            // Act
            let response = client
                .get(format!("{base_url}{path}"))
                .header("cookie", &admin_cookie)
                .send()
                .await
                .expect("method request");
            let status = response.status();
            let allow = response
                .headers()
                .get(reqwest::header::ALLOW)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let payload = response.json::<serde_json::Value>().await.expect("json");

            // Assert
            assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
            assert_eq!(allow, "POST");
            assert_eq!(payload["error"], "method not allowed");
        }

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
