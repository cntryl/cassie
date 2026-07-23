use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::CassieRuntimeConfig;
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use tokio_postgres::NoTls;
use uuid::Uuid;

fn fixture(label: &str) -> (Cassie, CassieSession, String) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = std::env::temp_dir()
        .join(format!(
            "cassie-database-connect-network-{label}-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("postgres", Some("postgres"), None)
        .expect("admin");
    for sql in [
        "CREATE DATABASE analytics",
        "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
        "GRANT CONNECT ON DATABASE analytics TO reader",
    ] {
        cassie
            .execute_sql(&admin, sql, Vec::new())
            .expect("database access fixture");
    }
    (cassie, admin, path)
}

async fn spawn_pgwire_server(
    cassie: Cassie,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<Result<(), CassieError>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire");
    let address = listener.local_addr().expect("pgwire address");
    drop(listener);
    let server = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        Arc::new(cassie),
        CassieRuntimeConfig::default(),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (address, server)
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
        .expect("bind REST");
    let address = listener.local_addr().expect("REST address");
    drop(listener);
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
        address.to_string(),
        cassie,
        Arc::clone(&shutdown),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (format!("http://{address}"), shutdown, server)
}

fn set_database_access(cassie: &Cassie, admin: &CassieSession, grant: bool) {
    let verb = if grant { "GRANT" } else { "REVOKE" };
    let preposition = if grant { "TO" } else { "FROM" };
    cassie
        .execute_sql(
            admin,
            &format!("{verb} CONNECT ON DATABASE analytics {preposition} reader"),
            Vec::new(),
        )
        .expect("change database access");
}

#[test]
fn should_revalidate_database_connect_for_an_active_pgwire_session() {
    // Arrange
    let (cassie, admin, path) = fixture("pgwire");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (address, server) = spawn_pgwire_server(cassie.clone()).await;
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(address.port())
            .user("reader")
            .password("reader-secret")
            .dbname("analytics");
        let (client, connection) = config.connect(NoTls).await.expect("reader connection");
        let connection = tokio::spawn(connection);
        client
            .simple_query("SELECT 1")
            .await
            .expect("granted query");

        // Act
        set_database_access(&cassie, &admin, false);
        let revoked = client
            .simple_query("SELECT 1")
            .await
            .expect_err("revoked query");
        set_database_access(&cassie, &admin, true);
        let restored = client.simple_query("SELECT 1").await;

        // Assert
        assert_eq!(
            revoked.as_db_error().map(|error| error.code().code()),
            Some("42501")
        );
        assert!(restored.is_ok());
        drop(client);
        connection.abort();
        server.abort();
        let _ = connection.await;
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_revalidate_database_connect_for_an_active_rest_session() {
    // Arrange
    let (cassie, admin, path) = fixture("rest");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie.clone()).await;
        let client = Client::new();
        let login = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "reader",
                "password": "reader-secret"
            }))
            .send()
            .await
            .expect("reader login");
        let cookie = login
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .expect("session cookie")
            .to_string();
        let query = || {
            client
                .post(format!("{base_url}/api/v1/admin/query-executions"))
                .header("cookie", &cookie)
                .json(&serde_json::json!({
                    "database": "analytics",
                    "sql": "SELECT 1"
                }))
                .send()
        };
        let granted = query().await.expect("granted query");

        // Act
        set_database_access(&cassie, &admin, false);
        let revoked = query().await.expect("revoked query");
        set_database_access(&cassie, &admin, true);
        let restored = query().await.expect("restored query");

        // Assert
        assert_eq!(granted.status(), StatusCode::OK);
        assert_eq!(revoked.status(), StatusCode::FORBIDDEN);
        assert_eq!(restored.status(), StatusCode::OK);
        shutdown.notify_waiters();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}
