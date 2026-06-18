#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use serde_json::json;
use tokio::process::{Child, Command};
use tokio_postgres::NoTls;
use uuid::Uuid;

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cassie-operational-smoke-{label}-{}",
        Uuid::new_v4()
    ))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local address")
        .port()
}

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_cassie")
}

fn spawn_cassie(data_dir: &Path, rest_port: u16, pgwire_port: u16) -> Child {
    let mut command = Command::new(binary_path());
    command
        .env("CASSIE_MIDGE_ALLOW_FALLBACK", "1")
        .env("CASSIE_MIDGE_DATA_DIR", data_dir)
        .env("CASSIE_REST_LISTEN", format!("127.0.0.1:{rest_port}"))
        .env("CASSIE_PGWIRE_LISTEN", format!("127.0.0.1:{pgwire_port}"))
        .env("CASSIE_ADMIN_USER", "postgres")
        .env("CASSIE_DEFAULT_DATABASE", "postgres")
        .env("CASSIE_ADMIN_PASSWORD", "")
        .env_remove("CASSIE_ADMIN_PASSWORD_FILE")
        .env("CASSIE_EMBEDDINGS_PROVIDER", "disabled")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().expect("spawn cassie binary")
}

async fn wait_for_ready(client: &reqwest::Client, base_url: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(response) = client.get(format!("{base_url}/health")).send().await {
                if response.status().is_success() {
                    if let Ok(body) = response.json::<serde_json::Value>().await {
                        if body["ready"].as_bool() == Some(true) {
                            assert_eq!(body["status"], "ok");
                            break;
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("cassie should become ready");
}

async fn terminate_cleanly(child: &mut Child) {
    let pid = child.id().expect("child pid");
    let status = StdCommand::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .expect("send SIGTERM");
    assert!(status.success(), "SIGTERM should be delivered successfully");

    let exit_status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("cassie should exit after SIGTERM")
        .expect("wait for cassie child");
    assert!(
        exit_status.success(),
        "cassie should exit cleanly after SIGTERM"
    );
}

async fn connect_pgwire(port: u16) -> (tokio_postgres::Client, tokio::task::JoinHandle<()>) {
    let mut config = tokio_postgres::Config::new();
    config.host("127.0.0.1");
    config.port(port);
    config.user("postgres");
    config.dbname("postgres");

    let (client, connection) = config.connect(NoTls).await.expect("connect pgwire");
    let connection = tokio::spawn(async move {
        connection
            .await
            .expect("pgwire connection task should stay healthy");
    });

    (client, connection)
}

#[test]
fn should_expose_health_liveness_through_the_binary() {
    // Arrange
    let path = data_dir("startup");
    let rest_port = free_port();
    let pgwire_port = free_port();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let client = reqwest::Client::new();
        let mut child = spawn_cassie(&path, rest_port, pgwire_port);
        let base_url = format!("http://127.0.0.1:{rest_port}");

        // Act
        wait_for_ready(&client, &base_url).await;
        let liveness = client
            .get(format!("{base_url}/liveness"))
            .send()
            .await
            .expect("liveness request");
        assert!(liveness.status().is_success());
        let liveness_json = liveness
            .json::<serde_json::Value>()
            .await
            .expect("liveness json");

        // Assert
        assert_eq!(liveness_json["ready"].as_bool(), Some(true));

        terminate_cleanly(&mut child).await;
        let _ = std::fs::remove_dir_all(&path);
    });
}

#[test]
fn should_restart_with_hydrated_catalog_through_the_binary() {
    // Arrange
    let path = data_dir("restart");
    let rest_port = free_port();
    let pgwire_port = free_port();
    let collection = format!("smoke_docs_{}", Uuid::new_v4().simple());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let client = reqwest::Client::new();
        let base_url = format!("http://127.0.0.1:{rest_port}");

        let mut child = spawn_cassie(&path, rest_port, pgwire_port);
        wait_for_ready(&client, &base_url).await;

        // Act
        let create = client
            .post(format!("{base_url}/v1/collections"))
            .json(&json!({
                "name": collection,
                "fields": [
                    {"name": "title", "type": "text"}
                ]
            }))
            .send()
            .await
            .expect("create collection request");
        assert!(create.status().is_success());
        let create_json = create
            .json::<serde_json::Value>()
            .await
            .expect("create json");
        assert_eq!(create_json["collection"], collection);

        let document = client
            .post(format!("{base_url}/v1/collections/{collection}/documents"))
            .json(&json!({"title": "alpha"}))
            .send()
            .await
            .expect("create document request");
        assert!(document.status().is_success());
        let document_json = document
            .json::<serde_json::Value>()
            .await
            .expect("document json");
        let document_id = document_json["id"]
            .as_str()
            .expect("document id present")
            .to_string();

        terminate_cleanly(&mut child).await;

        let restart_client = reqwest::Client::new();
        let mut child = spawn_cassie(&path, rest_port, pgwire_port);
        wait_for_ready(&restart_client, &base_url).await;

        let (pg_client, pg_connection) = connect_pgwire(pgwire_port).await;
        let row = tokio::time::timeout(
            Duration::from_secs(5),
            pg_client.query_one(
                &format!("SELECT title FROM {collection} ORDER BY title"),
                &[],
            ),
        )
        .await
        .expect("pgwire query should complete")
        .expect("pgwire query row");
        let title: String = row.try_get(0).expect("title column");

        let get = restart_client
            .get(format!(
                "{base_url}/v1/collections/{collection}/documents/{document_id}"
            ))
            .send()
            .await
            .expect("get document request");
        assert!(get.status().is_success());
        let get_json = get
            .json::<serde_json::Value>()
            .await
            .expect("document json");

        // Assert
        assert_eq!(title, "alpha");
        assert_eq!(get_json["title"], "alpha");

        pg_connection.abort();
        let _ = pg_connection.await;

        terminate_cleanly(&mut child).await;
        let _ = std::fs::remove_dir_all(&path);
    });
}
