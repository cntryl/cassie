use std::path::PathBuf;
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

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
