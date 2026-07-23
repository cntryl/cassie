use std::path::PathBuf;
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-rest-sessions-{label}-{}", Uuid::new_v4()))
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

fn cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get("set-cookie")
        .expect("session cookie")
        .to_str()
        .expect("cookie header")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_string()
}

#[test]
fn should_reject_invalid_rest_login_without_issuing_a_cookie() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("invalid-login");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "wrong"
            }))
            .send()
            .await
            .expect("login response");

        // Assert
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get("set-cookie").is_none());
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_revoke_cookie_session_after_logout() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("session-lifecycle");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let login = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "postgres"
            }))
            .send()
            .await
            .expect("login response");
        let session_cookie = cookie(&login);
        let set_cookie = login
            .headers()
            .get("set-cookie")
            .expect("set-cookie")
            .to_str()
            .expect("set-cookie value");

        // Act
        let current = client
            .get(format!("{base_url}/api/v1/auth/session"))
            .header("cookie", &session_cookie)
            .send()
            .await
            .expect("current session response");
        let logout = client
            .post(format!("{base_url}/api/v1/auth/logout"))
            .header("cookie", &session_cookie)
            .send()
            .await
            .expect("logout response");
        let after_logout = client
            .get(format!("{base_url}/api/v1/auth/session"))
            .header("cookie", &session_cookie)
            .send()
            .await
            .expect("post-logout response");

        // Assert
        assert_eq!(login.status(), StatusCode::OK);
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        assert_eq!(current.status(), StatusCode::OK);
        assert_eq!(logout.status(), StatusCode::OK);
        assert!(logout
            .headers()
            .get("set-cookie")
            .expect("clear cookie")
            .to_str()
            .expect("clear cookie value")
            .contains("Max-Age=0"));
        assert_eq!(after_logout.status(), StatusCode::UNAUTHORIZED);
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_reject_password_bearing_bearer_credentials() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("bearer-rejected");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();

        // Act
        let response = client
            .get(format!("{base_url}/api/v1/auth/session"))
            .header("authorization", "Bearer postgres:postgres")
            .send()
            .await
            .expect("bearer response");

        // Assert
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_ignore_forwarding_headers_when_deriving_plaintext_cookie_security() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("forwarded-headers");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .header("forwarded", "proto=https")
            .header("x-forwarded-proto", "https")
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "postgres"
            }))
            .send()
            .await
            .expect("login response");

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("Secure")));
        assert!(!response.headers().contains_key("strict-transport-security"));
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_secure_external_https_login_logout_cookies() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("external-https");
    let config = CassieRuntimeConfig {
        rest_external_https: true,
        ..CassieRuntimeConfig::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&data_dir, config).expect("configured cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let login = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "postgres"
            }))
            .send()
            .await
            .expect("login response");
        let session_cookie = cookie(&login);

        // Act
        let logout = client
            .post(format!("{base_url}/api/v1/auth/logout"))
            .header("cookie", session_cookie)
            .send()
            .await
            .expect("logout response");

        // Assert
        assert!(login
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("Secure")));
        assert!(login.headers().contains_key("strict-transport-security"));
        assert!(logout
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("Max-Age=0") && value.contains("Secure")));
        assert!(logout.headers().contains_key("strict-transport-security"));
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_refund_success_before_throttling_excess_rest_logins() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("login-throttle");
    let config = CassieRuntimeConfig {
        auth_user_attempts_per_minute: 1,
        auth_ip_attempts_per_minute: 10,
        ..CassieRuntimeConfig::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&data_dir, config).expect("configured cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let valid_login = || {
            client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "postgres",
                    "password": "postgres"
                }))
                .send()
        };

        // Act
        let first_success = valid_login().await.expect("first success");
        let second_success = valid_login().await.expect("refunded success");
        let invalid = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "POSTGRES",
                "password": "wrong"
            }))
            .send()
            .await
            .expect("invalid login");
        let throttled = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "wrong"
            }))
            .send()
            .await
            .expect("throttled login");

        // Assert
        assert_eq!(first_success.status(), StatusCode::OK);
        assert_eq!(second_success.status(), StatusCode::OK);
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            throttled
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
            Some("60")
        );
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_map_oversized_rest_sql_to_bad_request() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("sql-resource-limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let login = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "postgres"
            }))
            .send()
            .await
            .expect("login response");
        let session_cookie = cookie(&login);

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", session_cookie)
            .json(&serde_json::json!({
                "database": "postgres",
                "sql": "x".repeat(1024 * 1024 + 1)
            }))
            .send()
            .await
            .expect("query response");
        let status = response.status();
        let body = response.text().await.expect("error body");

        // Assert
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("SQL text exceeds"));
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}
