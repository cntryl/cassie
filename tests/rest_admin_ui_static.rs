use std::path::PathBuf;
use std::sync::Arc;

use cassie::app::Cassie;
use tokio::sync::Notify;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn temp_path(label: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-admin-ui-{label}-{}", Uuid::new_v4()));
    path
}

fn data_dir(label: &str) -> String {
    temp_path(label).to_string_lossy().to_string()
}

async fn login_cookie(client: &reqwest::Client, base_url: &str) -> String {
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

fn write_dist_fixture(label: &str) -> PathBuf {
    let dist = temp_path(label);
    std::fs::create_dir_all(dist.join("assets")).expect("create assets dir");
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><html><body><div id=\"app\">Cassie Admin Shell</div></body></html>",
    )
    .expect("write index");
    std::fs::write(
        dist.join("assets").join("app.js"),
        "console.log('cassie admin asset');",
    )
    .expect("write asset");
    std::fs::write(
        dist.join("assets").join("app-AbCd1234.js"),
        "console.log('hashed cassie admin asset');",
    )
    .expect("write hashed asset");
    dist
}

async fn spawn_rest_server(
    cassie: Cassie,
    admin_ui_dir: PathBuf,
) -> (
    String,
    Arc<Notify>,
    tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    drop(listener);

    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(cassie::rest::router::run_with_shutdown_and_admin_ui_dir(
        addr.to_string(),
        cassie,
        shutdown.clone(),
        admin_ui_dir,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (format!("http://{addr}"), shutdown, server)
}

async fn stop_rest_server(
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
) {
    shutdown.notify_waiters();
    let _ = server.await;
}

#[test]
fn should_serve_admin_index_for_shell_routes() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("admin-index");
    let dist = write_dist_fixture("admin-index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let client = reqwest::Client::new();

        // Act
        let admin = client
            .get(format!("{base_url}/"))
            .send()
            .await
            .expect("admin request");
        let deep_link = client
            .get(format!("{base_url}/catalog"))
            .send()
            .await
            .expect("deep link request");
        let admin_content_type = admin
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let admin_body = admin.text().await.expect("admin body");
        let deep_link_body = deep_link.text().await.expect("deep link body");

        // Assert
        assert!(admin_content_type.contains("text/html"));
        assert!(admin_body.contains("Cassie Admin Shell"));
        assert!(deep_link_body.contains("Cassie Admin Shell"));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_serve_built_admin_assets() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("admin-asset");
    let dist = write_dist_fixture("admin-asset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let client = reqwest::Client::new();

        // Act
        let asset = client
            .get(format!("{base_url}/assets/app.js"))
            .send()
            .await
            .expect("asset request");
        let asset_status = asset.status();
        let asset_content_type = asset
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let asset_body = asset.text().await.expect("asset body");
        let hashed_asset = client
            .get(format!("{base_url}/assets/app-AbCd1234.js"))
            .send()
            .await
            .expect("hashed asset request");

        // Assert
        assert!(asset_status.is_success());
        assert!(asset_content_type.contains("javascript"));
        assert!(asset_body.contains("cassie admin asset"));
        assert_eq!(
            hashed_asset.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_return_not_found_when_admin_ui_dir_is_missing() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("missing-admin-ui");
    let missing_dist = temp_path("missing-admin-ui");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, missing_dist).await;
        let client = reqwest::Client::new();

        // Act
        let response = client
            .get(format!("{base_url}/"))
            .send()
            .await
            .expect("admin request");

        // Assert
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
}

#[test]
fn should_reject_admin_asset_path_traversal() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("admin-traversal");
    let dist = write_dist_fixture("admin-traversal");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let host = base_url.strip_prefix("http://").expect("base URL host");
        let mut stream = tokio::net::TcpStream::connect(host)
            .await
            .expect("connect to rest server");
        let request = format!(
            "GET /assets/../Cargo.toml HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        );

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
            .await
            .expect("write traversal request");
        let mut response = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
            .await
            .expect("read traversal response");

        // Assert
        assert!(response.starts_with("HTTP/1.1 404"));
        assert!(!response.contains("[package]"));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_preserve_existing_route_auth_behavior() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("existing-routes");
    let dist = write_dist_fixture("existing-routes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let client = reqwest::Client::new();

        // Act
        let health = client
            .get(format!("{base_url}/health"))
            .send()
            .await
            .expect("health request");
        let liveness = client
            .get(format!("{base_url}/liveness"))
            .send()
            .await
            .expect("liveness request");
        let targetz = client
            .get(format!("{base_url}/targetz"))
            .send()
            .await
            .expect("targetz request");
        let metrics = client
            .get(format!("{base_url}/metrics"))
            .send()
            .await
            .expect("metrics request");
        let collections = client
            .get(format!("{base_url}/api/v1/collections"))
            .send()
            .await
            .expect("collections request");

        // Assert
        assert!(health.status().is_success());
        assert!(liveness.status().is_success());
        assert!(targetz.status().is_success());
        assert!(metrics.status().is_success());
        assert_eq!(collections.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(collections.headers()["cache-control"], "no-store");
        assert_eq!(collections.headers()["x-content-type-options"], "nosniff");
        assert_eq!(collections.headers()["x-frame-options"], "DENY");
        assert_eq!(collections.headers()["referrer-policy"], "no-referrer");
        assert!(collections.headers()["content-security-policy"]
            .to_str()
            .expect("CSP header")
            .contains("default-src"));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_reject_oversized_rest_request_body() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("oversized-body");
    let dist = write_dist_fixture("oversized-body");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let client = reqwest::Client::new();
        let body = vec![b'x'; 8 * 1024 * 1024 + 1];

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .expect("oversized request");

        // Assert
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_reject_non_json_rest_write_requests() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("non-json-body");
    let dist = write_dist_fixture("non-json-body");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        // Act
        let response = reqwest::Client::new()
            .post(format!("{base_url}/api/v1/auth/login"))
            .body("username=postgres&password=postgres")
            .send()
            .await
            .expect("non-JSON request");

        // Assert
        assert_eq!(
            response.status(),
            reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_reject_cross_origin_rest_state_changes() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("cross-origin");
    let dist = write_dist_fixture("cross-origin");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        // Act
        let response = reqwest::Client::new()
            .post(format!("{base_url}/api/v1/auth/login"))
            .header("origin", "https://evil.example")
            .json(&serde_json::json!({
                "username": "postgres",
                "password": "postgres"
            }))
            .send()
            .await
            .expect("cross-origin request");

        // Assert
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}

#[test]
fn should_not_serve_admin_shell_for_unmatched_api_routes() {
    // Arrange
    with_fallback();
    let data_dir = data_dir("unmatched-api");
    let dist = write_dist_fixture("unmatched-api");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let client = reqwest::Client::new();
        let session_cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .get(format!("{base_url}/api/v1/does-not-exist"))
            .header("cookie", &session_cookie)
            .send()
            .await
            .expect("unmatched api request");
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        // Assert
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        assert!(content_type.contains("application/json"));

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
}
