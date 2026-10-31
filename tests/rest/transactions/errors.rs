//! REST mapping of typed transaction errors reachable from one request session.

use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

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
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
        address.to_string(),
        cassie,
        shutdown.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    (format!("http://{address}"), shutdown, server)
}

async fn login_cookie(client: &Client, base_url: &str) -> String {
    client
        .post(format!("{base_url}/api/v1/auth/login"))
        .json(&serde_json::json!({ "username": "root", "password": crate::support_sql::test_root_password() }))
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
fn should_map_idle_savepoint_controls_to_rest_bad_requests() {
    // Arrange
    crate::support_temp_dirs::sweep_stale_once();
    let data_dir = std::env::temp_dir().join(format!("cassie-rest-tx-errors-{}", Uuid::new_v4()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let responses = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url).await;

        // Act
        let mut responses = Vec::new();
        for sql in [
            "SAVEPOINT idle_point",
            "ROLLBACK TO SAVEPOINT idle_point",
            "RELEASE SAVEPOINT idle_point",
        ] {
            let response = client
                .post(format!("{base_url}/api/v1/admin/query/execute"))
                .header("cookie", &cookie)
                .json(&serde_json::json!({ "database": "postgres", "sql": sql }))
                .send()
                .await
                .expect("execute request");
            let status = response.status();
            let payload = response
                .json::<serde_json::Value>()
                .await
                .expect("error payload");
            responses.push((status, payload["error"].as_str().map(str::to_string)));
        }
        shutdown.notify_waiters();
        let _ = server.await;
        responses
    });
    let _ = std::fs::remove_dir_all(&data_dir);

    // Assert
    assert_eq!(
        responses,
        [
            "SAVEPOINT can only be used in transaction blocks",
            "ROLLBACK TO SAVEPOINT can only be used in transaction blocks",
            "RELEASE SAVEPOINT can only be used in transaction blocks",
        ]
        .map(|message| (StatusCode::BAD_REQUEST, Some(message.to_string())))
    );
}
