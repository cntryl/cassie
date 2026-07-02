#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use uuid::Uuid;

struct JsonHttpResponse {
    status: u16,
    body: serde_json::Value,
}

impl JsonHttpResponse {
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

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
    command.kill_on_drop(true);
    command.spawn().expect("spawn cassie binary")
}

async fn wait_for_ready(child: &mut Child, base_url: &str) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(status) = child.try_wait().expect("poll cassie child") {
                panic!("cassie exited before becoming ready: {status}");
            }

            if let Ok(response) = request_json("GET", base_url, "/health", None).await {
                if response.is_success() && response.body["ready"].as_bool() == Some(true) {
                    assert_eq!(response.body["status"], "ok");
                    break;
                }
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("cassie should become ready");
}

async fn request_json(
    method: &str,
    base_url: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<JsonHttpResponse, String> {
    let (host, port) = parse_localhost_url(base_url)?;
    let mut stream = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|error| format!("connect {base_url}: {error}"))?;
    let body = body
        .map(|value| serde_json::to_vec(&value).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    let content_type = if body.is_empty() {
        ""
    } else {
        "Content-Type: application/json\r\n"
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n{content_type}Content-Length: {}\r\n\r\n",
        body.len()
    );

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| format!("write request: {error}"))?;
    stream
        .write_all(&body)
        .await
        .map_err(|error| format!("write body: {error}"))?;

    let mut raw_response = Vec::new();
    stream
        .read_to_end(&mut raw_response)
        .await
        .map_err(|error| format!("read response: {error}"))?;
    parse_json_response(&raw_response)
}

fn parse_localhost_url(base_url: &str) -> Result<(String, u16), String> {
    let host_port = base_url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported base URL '{base_url}'"))?;
    let (host, port) = host_port
        .rsplit_once(':')
        .ok_or_else(|| format!("missing port in base URL '{base_url}'"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| format!("invalid port in base URL '{base_url}': {error}"))?;
    Ok((host.to_string(), port))
}

fn parse_json_response(raw_response: &[u8]) -> Result<JsonHttpResponse, String> {
    let response =
        std::str::from_utf8(raw_response).map_err(|error| format!("response utf8: {error}"))?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "response missing header terminator".to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "response missing status".to_string())?
        .parse::<u16>()
        .map_err(|error| format!("invalid status: {error}"))?;
    let body = serde_json::from_str(body).map_err(|error| format!("response json: {error}"))?;
    Ok(JsonHttpResponse { status, body })
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

async fn pgwire_query_one_text(port: u16, sql: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect pgwire");
    write_pg_startup(&mut stream).await;
    wait_for_pg_ready(&mut stream).await;
    write_pg_query(&mut stream, sql).await;
    let value = read_pg_query_text(&mut stream).await;
    write_pg_terminate(&mut stream).await;
    value
}

async fn write_pg_startup(stream: &mut TcpStream) {
    let mut payload = Vec::new();
    payload.extend_from_slice(&196_608_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0postgres\0database\0postgres\0\0");
    write_pg_untagged(stream, &payload).await;
}

async fn write_pg_query(stream: &mut TcpStream, sql: &str) {
    let mut payload = Vec::from(sql.as_bytes());
    payload.push(0);
    write_pg_tagged(stream, b'Q', &payload).await;
}

async fn write_pg_terminate(stream: &mut TcpStream) {
    write_pg_tagged(stream, b'X', &[]).await;
}

async fn write_pg_untagged(stream: &mut TcpStream, payload: &[u8]) {
    let length = i32::try_from(payload.len() + 4).expect("pgwire message length");
    stream
        .write_all(&length.to_be_bytes())
        .await
        .expect("write pgwire length");
    stream
        .write_all(payload)
        .await
        .expect("write pgwire payload");
}

async fn write_pg_tagged(stream: &mut TcpStream, tag: u8, payload: &[u8]) {
    stream.write_all(&[tag]).await.expect("write pgwire tag");
    write_pg_untagged(stream, payload).await;
}

async fn wait_for_pg_ready(stream: &mut TcpStream) {
    loop {
        let (tag, payload) = read_pg_message(stream).await;
        match tag {
            b'R' => assert_eq!(read_i32(&payload, 0), 0, "pgwire auth should be ok"),
            b'E' => panic!("pgwire startup error: {}", pg_error_message(&payload)),
            b'Z' => break,
            _ => {}
        }
    }
}

async fn read_pg_query_text(stream: &mut TcpStream) -> String {
    let mut first_value = None;
    loop {
        let (tag, payload) = read_pg_message(stream).await;
        match tag {
            b'D' => {
                if first_value.is_none() {
                    first_value = Some(read_first_data_row_value(&payload));
                }
            }
            b'E' => panic!("pgwire query error: {}", pg_error_message(&payload)),
            b'Z' => return first_value.expect("pgwire row"),
            _ => {}
        }
    }
}

async fn read_pg_message(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut tag = [0_u8; 1];
    stream.read_exact(&mut tag).await.expect("read pgwire tag");
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .await
        .expect("read pgwire length");
    let payload_len = u32::from_be_bytes(length)
        .checked_sub(4)
        .expect("pgwire payload length");
    let payload_len = usize::try_from(payload_len).expect("pgwire payload length usize");
    let mut payload = vec![0_u8; payload_len];
    stream
        .read_exact(&mut payload)
        .await
        .expect("read pgwire payload");
    (tag[0], payload)
}

fn read_first_data_row_value(payload: &[u8]) -> String {
    let mut offset = 0;
    assert_eq!(read_u16_at(payload, &mut offset), 1, "pgwire column count");
    let length = read_i32_at(payload, &mut offset);
    assert!(length >= 0, "pgwire value should not be null");
    let length = usize::try_from(length).expect("pgwire value length");
    let end = offset + length;
    let value = payload.get(offset..end).expect("pgwire value bytes");
    std::str::from_utf8(value)
        .expect("pgwire value utf8")
        .to_string()
}

fn read_u16_at(payload: &[u8], offset: &mut usize) -> u16 {
    let end = *offset + 2;
    let bytes = payload.get(*offset..end).expect("pgwire u16");
    *offset = end;
    u16::from_be_bytes(bytes.try_into().expect("pgwire u16 bytes"))
}

fn read_i32_at(payload: &[u8], offset: &mut usize) -> i32 {
    let value = read_i32(payload, *offset);
    *offset += 4;
    value
}

fn read_i32(payload: &[u8], offset: usize) -> i32 {
    let end = offset + 4;
    let bytes = payload.get(offset..end).expect("pgwire i32");
    i32::from_be_bytes(bytes.try_into().expect("pgwire i32 bytes"))
}

fn pg_error_message(payload: &[u8]) -> String {
    let fields = payload
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect::<Vec<_>>();
    fields.join("; ")
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
        let mut child = spawn_cassie(&path, rest_port, pgwire_port);
        let base_url = format!("http://127.0.0.1:{rest_port}");

        // Act
        wait_for_ready(&mut child, &base_url).await;
        let liveness = request_json("GET", &base_url, "/liveness", None)
            .await
            .expect("liveness request");
        assert!(liveness.is_success());

        // Assert
        assert_eq!(liveness.body["ready"].as_bool(), Some(true));

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
        let base_url = format!("http://127.0.0.1:{rest_port}");

        let mut child = spawn_cassie(&path, rest_port, pgwire_port);
        wait_for_ready(&mut child, &base_url).await;

        // Act
        let create = request_json(
            "POST",
            &base_url,
            "/v1/collections",
            Some(json!({
                "name": collection,
                "fields": [
                    {"name": "title", "type": "text"}
                ]
            })),
        )
        .await
        .expect("create collection request");
        assert!(create.is_success());
        assert_eq!(create.body["collection"], collection);

        let document = request_json(
            "POST",
            &base_url,
            &format!("/v1/collections/{collection}/documents"),
            Some(json!({"title": "alpha"})),
        )
        .await
        .expect("create document request");
        assert!(document.is_success());
        let document_id = document.body["id"]
            .as_str()
            .expect("document id present")
            .to_string();

        terminate_cleanly(&mut child).await;

        let mut child = spawn_cassie(&path, rest_port, pgwire_port);
        wait_for_ready(&mut child, &base_url).await;

        let title = tokio::time::timeout(
            Duration::from_secs(5),
            pgwire_query_one_text(
                pgwire_port,
                &format!("SELECT title FROM {collection} ORDER BY title"),
            ),
        )
        .await
        .expect("pgwire query should complete");

        let get = request_json(
            "GET",
            &base_url,
            &format!("/v1/collections/{collection}/documents/{document_id}"),
            None,
        )
        .await
        .expect("get document request");
        assert!(get.is_success());

        // Assert
        assert_eq!(title, "alpha");
        assert_eq!(get.body["title"], "alpha");

        terminate_cleanly(&mut child).await;
        let _ = std::fs::remove_dir_all(&path);
    });
}
