use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

type QueryEndpointCase = (reqwest::Method, String, Option<serde_json::Value>);

fn use_local_storage() {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
}

fn data_dir(label: &str) -> PathBuf {
    crate::support_temp_dirs::sweep_stale_once();
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
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin session");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_admin_query_docs (id INT PRIMARY KEY, title TEXT)",
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
            format!("{base_url}/api/v1/admin/query/schema?database=postgres"),
            None,
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query/execute"),
            Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query/validate"),
            Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query/explain"),
            Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::GET,
            format!("{base_url}/api/v1/admin/catalog?database=postgres"),
            None,
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query-executions"),
            Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query-validations"),
            Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
        ),
        (
            reqwest::Method::POST,
            format!("{base_url}/api/v1/admin/query-explanations"),
            Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
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
            "username": "root",
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
        .json(&serde_json::json!({ "database": "postgres", "sql": sql }))
        .send()
        .await
        .expect("admin query request")
}

include!("semantics.rs");

include!("errors.rs");

include!("cancellation.rs");
