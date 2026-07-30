use std::path::PathBuf;
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::catalog::canonical_schema_name;
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

fn use_local_storage() {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
}

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cassie-rest-admin-databases-{label}-{}",
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

async fn stop_rest_server(
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
) {
    shutdown.notify_waiters();
    let _ = server.await;
}

async fn login_cookie(
    client: &Client,
    base_url: &str,
    username: &str,
    password: &str,
) -> String {
    client
        .post(format!("{base_url}/api/v1/auth/login"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
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
fn should_create_admin_database_through_dedicated_rest_endpoint() {
    // Arrange
    use_local_storage();
    let path = data_dir("create");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let observable = cassie.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url, "root", "postgres").await;

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/admin/databases"))
            .header("cookie", cookie)
            .json(&serde_json::json!({ "name": " Analytics_1 " }))
            .send()
            .await
            .expect("create database");

        // Assert
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response
                .json::<serde_json::Value>()
                .await
                .expect("database summary"),
            serde_json::json!({ "name": "analytics_1" })
        );
        assert!(observable.catalog.database_exists("analytics_1"));
        assert!(observable
            .catalog
            .namespace_exists(&canonical_schema_name("analytics_1", "public")));
        assert!(observable
            .midge
            .get_database("analytics_1")
            .expect("database metadata")
            .is_some());
        assert!(observable
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "analytics_1.public"));

        stop_rest_server(shutdown, server).await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_duplicate_admin_database_names() {
    // Arrange
    use_local_storage();
    let path = data_dir("duplicate");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url, "root", "postgres").await;
        let first = client
            .post(format!("{base_url}/api/v1/admin/databases"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({ "name": "analytics" }))
            .send()
            .await
            .expect("first create database");
        assert_eq!(first.status(), StatusCode::CREATED);

        // Act
        let duplicate = client
            .post(format!("{base_url}/api/v1/admin/databases"))
            .header("cookie", cookie)
            .json(&serde_json::json!({ "name": "analytics" }))
            .send()
            .await
            .expect("duplicate create database");

        // Assert
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        assert_eq!(
            duplicate
                .json::<serde_json::Value>()
                .await
                .expect("duplicate error")["error"],
            "database 'analytics' already exists"
        );

        stop_rest_server(shutdown, server).await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_admin_database_names() {
    // Arrange
    use_local_storage();
    let path = data_dir("invalid");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url, "root", "postgres").await;

        // Act
        let mut statuses = Vec::new();
        for name in ["", "tenant.analytics", "9analytics", "analytics-reporting"] {
            statuses.push(
                client
                    .post(format!("{base_url}/api/v1/admin/databases"))
                    .header("cookie", &cookie)
                    .json(&serde_json::json!({ "name": name }))
                    .send()
                    .await
                    .expect("invalid create database")
                    .status(),
            );
        }

        // Assert
        assert_eq!(statuses, vec![StatusCode::BAD_REQUEST; 4]);

        stop_rest_server(shutdown, server).await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_require_admin_authorization_to_create_database() {
    // Arrange
    use_local_storage();
    let path = data_dir("authorization");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let root = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("root session");
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
            Vec::new(),
        )
        .expect("create reader");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let reader_cookie =
            login_cookie(&client, &base_url, "reader", "reader-secret").await;

        // Act
        let unauthorized = client
            .post(format!("{base_url}/api/v1/admin/databases"))
            .json(&serde_json::json!({ "name": "unauthorized_database" }))
            .send()
            .await
            .expect("unauthorized create database");
        let forbidden = client
            .post(format!("{base_url}/api/v1/admin/databases"))
            .header("cookie", reader_cookie)
            .json(&serde_json::json!({ "name": "forbidden_database" }))
            .send()
            .await
            .expect("forbidden create database");

        // Assert
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        stop_rest_server(shutdown, server).await;
    });

    let _ = std::fs::remove_dir_all(path);
}
