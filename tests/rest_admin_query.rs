use std::path::PathBuf;
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

const ADMIN_AUTH: &str = "Bearer postgres:postgres";
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
            format!("{base_url}/v1/admin/query/schema"),
            None,
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/v1/admin/query/execute"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/v1/admin/query/validate"),
            Some(serde_json::json!({"sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/v1/admin/query/explain"),
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

        // Act
        let response = client
            .post(format!("{base_url}/v1/admin/query/execute"))
            .header("authorization", ADMIN_AUTH)
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

        // Act
        let valid = client
            .post(format!("{base_url}/v1/admin/query/validate"))
            .header("authorization", ADMIN_AUTH)
            .json(&serde_json::json!({
                "sql": "SELECT title FROM rest_admin_query_docs"
            }))
            .send()
            .await
            .expect("valid request");
        let valid_status = valid.status();
        let valid_payload = valid.json::<serde_json::Value>().await.expect("valid json");
        let malformed = client
            .post(format!("{base_url}/v1/admin/query/validate"))
            .header("authorization", ADMIN_AUTH)
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

        // Act
        let response = client
            .post(format!("{base_url}/v1/admin/query/explain"))
            .header("authorization", ADMIN_AUTH)
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

        // Act
        let response = client
            .get(format!("{base_url}/v1/admin/query/schema"))
            .header("authorization", ADMIN_AUTH)
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
            "rest_admin_query_docs"
        ));
        assert!(contains_item(
            section_items(&payload, "views"),
            "rest_admin_query_ready"
        ));
        assert!(contains_item(
            section_items(&payload, "indexes"),
            "rest_admin_query_title_idx"
        ));
        assert!(contains_item(
            section_items(&payload, "udfs"),
            "rest_query_identity"
        ));
        assert!(contains_item(
            section_items(&payload, "procedures"),
            "rest_query_store"
        ));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_return_method_not_allowed_for_known_admin_query_path() {
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

        // Act
        let response = client
            .get(format!("{base_url}/v1/admin/query/execute"))
            .header("authorization", ADMIN_AUTH)
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

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
