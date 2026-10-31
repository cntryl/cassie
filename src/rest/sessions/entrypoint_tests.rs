//! Opaque-token invalidation through the existing HTTP SQL execution endpoint.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::{
    issue_with_limits, revoke, session_key, PersistedSession, MAX_SESSIONS, SESSION_TTL_SECONDS,
};
use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::midge::StorageFamily;
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;

fn test_root_password() -> &'static str {
    static TEST_ROOT_PASSWORD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TEST_ROOT_PASSWORD
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .as_str()
}

#[derive(Clone, Copy)]
enum Transition {
    Revoke,
    Rotate,
    DeleteRole,
    NoLogin,
    Expire,
}

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("session fixture cleanup failed during unwind: {error}");
            } else {
                panic!("session fixture cleanup failed: {error}");
            }
        }
    }
}

struct Fixture {
    cassie: Cassie,
    _directory: Directory,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-rest-session-query-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).expect("fixture directory");
        let directory = Directory(path);
        let config = CassieRuntimeConfig {
            password: test_root_password().to_owned(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&directory.0, config).expect("Cassie");
        cassie.startup().expect("startup");
        Self {
            cassie,
            _directory: directory,
        }
    }
}

struct Response {
    status: StatusCode,
    body: serde_json::Value,
}

struct AfterTransition {
    denied: Response,
    control: Response,
    before_count: u64,
    denied_count: u64,
    control_count: u64,
    record_removed: bool,
}

struct Observations {
    before: Response,
    after: Option<AfterTransition>,
}

fn setup(cassie: &Cassie) -> (CassieSession, String) {
    let admin = cassie
        .authenticate_role("root", Some(test_root_password()), None)
        .expect("root role");
    for sql in [
        "CREATE ROLE entry_reader LOGIN PASSWORD 'entry-reader-password'",
        "GRANT CONNECT ON DATABASE postgres TO entry_reader",
    ] {
        cassie
            .execute_sql(&admin, sql, Vec::new())
            .expect("reader fixture SQL");
    }
    let principal = cassie
        .authenticate_principal("entry_reader", Some("entry-reader-password"), None)
        .expect("reader principal");
    let token = issue_with_limits(cassie, &principal.role, MAX_SESSIONS, SESSION_TTL_SECONDS)
        .expect("valid opaque reader token");
    (admin, token)
}

fn transition(cassie: &Cassie, admin: &CassieSession, token: &str, change: Transition) {
    match change {
        Transition::Revoke => revoke(cassie, token).expect("revoke token"),
        Transition::Rotate => {
            cassie
                .execute_sql(
                    admin,
                    "ALTER ROLE entry_reader PASSWORD 'rotated-reader-password'",
                    Vec::new(),
                )
                .expect("rotate reader password");
        }
        Transition::DeleteRole => cassie
            .drop_role("entry_reader", false)
            .expect("delete reader role"),
        Transition::NoLogin => {
            cassie
                .execute_sql(admin, "ALTER ROLE entry_reader NOLOGIN", Vec::new())
                .expect("disable login for reader role");
        }
        Transition::Expire => {
            let key = session_key(token);
            let raw = cassie
                .midge
                .raw_get(StorageFamily::Schema, &key)
                .expect("read token record")
                .expect("token exists before expiry injection");
            let mut record: PersistedSession =
                serde_json::from_slice(&raw).expect("existing record");
            record.expires_at = 0;
            cassie
                .midge
                .raw_put(
                    StorageFamily::Schema,
                    &key,
                    &serde_json::to_vec(&record).expect("existing record encoding"),
                )
                .expect("inject already expired timestamp without a clock hook or sleep");
        }
    }
}

fn count(cassie: &Cassie) -> u64 {
    cassie.metrics()["query"]["count"]
        .as_u64()
        .expect("successful SQL query count")
}

async fn query(client: &Client, base_url: &str, token: &str) -> Response {
    let response = client
        .post(format!("{base_url}/api/v1/admin/query-executions"))
        .header("cookie", format!("{}={token}", super::SESSION_COOKIE))
        .header("connection", "close")
        .json(&serde_json::json!({"database": "postgres", "sql": "SELECT 1 AS marker"}))
        .send()
        .await
        .expect("HTTP SQL request");
    let status = response.status();
    let body = response.json().await.expect("complete HTTP response body");
    Response { status, body }
}

async fn observe_transition(
    cassie: &Cassie,
    client: &Client,
    base_url: &str,
    admin: &CassieSession,
    token: &str,
    change: Transition,
) -> AfterTransition {
    transition(cassie, admin, token, change);
    let before_count = count(cassie);
    let denied = query(client, base_url, token).await;
    let denied_count = count(cassie);
    // Observe removal before issuing another token can independently purge expiry.
    let record_removed = cassie
        .midge
        .raw_get(StorageFamily::Schema, &session_key(token))
        .expect("read old token after denied HTTP request")
        .is_none();
    let root = cassie
        .authenticate_principal("root", Some(test_root_password()), None)
        .expect("fresh root principal");
    let fresh = issue_with_limits(cassie, &root.role, MAX_SESSIONS, SESSION_TTL_SECONDS)
        .expect("fresh root token");
    let control = query(client, base_url, &fresh).await;
    let control_count = count(cassie);
    AfterTransition {
        denied,
        control,
        before_count,
        denied_count,
        control_count,
        record_removed,
    }
}

async fn observe(cassie: &Cassie, change: Transition) -> Observations {
    let (admin, token) = setup(cassie);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral address");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let base_url = format!("http://{address}");
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(crate::rest::router::run_with_shutdown(
        address.to_string(),
        cassie.clone(),
        Arc::clone(&shutdown),
    ));
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(0)
        .build()
        .expect("HTTP client");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(response) = client
                .get(format!("{base_url}/health"))
                .header("connection", "close")
                .send()
                .await
            {
                if response.status() == StatusCode::OK {
                    let _ = response.bytes().await.expect("health response body");
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual HTTP listener becomes ready without a sleep");
    let before = query(&client, &base_url, &token).await;
    // Do not run dependent invalidation probes if the actual valid request failed.
    let after =
        if before.status == StatusCode::OK && before.body["rows"] == serde_json::json!([[1]]) {
            Some(observe_transition(cassie, &client, &base_url, &admin, &token, change).await)
        } else {
            None
        };
    drop(client);
    shutdown.notify_one();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server stops")
        .expect("server task joins")
        .expect("server shutdown succeeds");
    Observations { before, after }
}

fn collect(change: Transition) -> Observations {
    let fixture = Fixture::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    let observations = runtime.block_on(observe(&fixture.cassie, change));
    drop(runtime);
    fixture.cassie.shutdown();
    drop(fixture);
    observations
}

fn assert_invalidated(observations: &Observations) {
    // All final assertions run after HTTP tasks, storage owners and directory retire.
    assert_eq!(
        observations.before.status,
        StatusCode::OK,
        "{:?}",
        observations.before.body
    );
    assert_eq!(observations.before.body["rows"], serde_json::json!([[1]]));
    let after = observations
        .after
        .as_ref()
        .expect("valid baseline enabled transition probe");
    assert_eq!(
        after.denied.status,
        StatusCode::UNAUTHORIZED,
        "{:?}",
        after.denied.body
    );
    assert_eq!(
        after.denied_count, after.before_count,
        "invalid token cannot complete a successful SQL query"
    );
    assert!(after.record_removed, "invalid token state has been retired");
    assert_eq!(
        after.control.status,
        StatusCode::OK,
        "{:?}",
        after.control.body
    );
    assert_eq!(after.control.body["rows"], serde_json::json!([[1]]));
    assert_eq!(after.control_count, after.denied_count + 1);
}

#[test]
fn should_reject_rest_query_after_opaque_session_revocation() {
    // Arrange
    let change = Transition::Revoke;
    // Act
    let observations = collect(change);
    // Assert
    assert_invalidated(&observations);
}

#[test]
fn should_reject_rest_query_after_role_password_rotation() {
    // Arrange
    let change = Transition::Rotate;
    // Act
    let observations = collect(change);
    // Assert
    assert_invalidated(&observations);
}

#[test]
fn should_reject_rest_query_after_login_role_deletion() {
    // Arrange
    let change = Transition::DeleteRole;
    // Act
    let observations = collect(change);
    // Assert
    assert_invalidated(&observations);
}

#[test]
fn should_reject_rest_query_after_injected_session_expiry() {
    // Arrange
    let change = Transition::Expire;
    // Act
    let observations = collect(change);
    // Assert
    assert_invalidated(&observations);
}

#[test]
fn should_reject_rest_query_after_login_role_is_disabled() {
    // Arrange
    let change = Transition::NoLogin;
    // Act
    let observations = collect(change);
    // Assert
    assert_invalidated(&observations);
}
