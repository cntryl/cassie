use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::openai::OpenAiConfig;
use cassie::embeddings::DEFAULT_EMBEDDING_MODEL;
use cassie::rest;
use uuid::Uuid;

#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: String,
}

struct MockOpenAiServer {
    base_url: String,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockOpenAiServer {
    fn spawn(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock openai");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("mock server addr")
        );
        let thread = thread::spawn(move || {
            let responses = responses.into_iter();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("mock accept");
                let body = read_http_body(&mut stream);
                if body.is_empty() {
                    continue;
                }

                let mut output = String::new();
                output.push_str("HTTP/1.1 ");
                output.push_str(&format!("{} OK\r\n", response.status));
                output.push_str("content-type: application/json\r\n");
                output.push_str(&format!("content-length: {}\r\n", response.body.len()));
                output.push_str("connection: close\r\n\r\n");
                output.push_str(&response.body);
                let _ = stream.write_all(output.as_bytes());
                let _ = stream.flush();
            }
        });

        Self {
            base_url,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        self.base_url.clone()
    }
}

impl Drop for MockOpenAiServer {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-rest-embeds-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn openai_runtime_with_server(base_url: String) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env();
    config.embeddings = EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
        config: OpenAiConfig {
            api_key: "test-key".to_string(),
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
        },
        timeout_seconds: 2,
        max_batch_size: 3,
        max_retries: 1,
        base_url: Some(base_url),
    });
    config
}

fn response_body(vectors: &[Vec<f32>]) -> String {
    let data: Vec<_> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| {
            serde_json::json!({
                "index": index,
                "embedding": vector,
            })
        })
        .collect();
    serde_json::json!({"data": data}).to_string()
}

#[test]
fn should_search_vector_docs_after_ingest() {
    // Arrange
    with_fallback();
    let path = data_dir("search_flow");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .unwrap();

    let openai_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: response_body(&[{ vec![0.0; 1536] }]),
        },
        MockResponse {
            status: 200,
            body: response_body(&[{
                let mut vector = vec![0.0; 1536];
                vector[0] = 5.0;
                vector
            }]),
        },
        MockResponse {
            status: 200,
            body: response_body(&[{
                let mut vector = vec![0.0; 1536];
                vector[0] = 2.0;
                vector
            }]),
        },
    ]);

    let server_base_url = openai_server.base_url();
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, openai_runtime_with_server(server_base_url))
            .unwrap();

    runtime.block_on(async {
        // Arrange
        cassie.startup().await.unwrap();

        rest::collections::create(
            &cassie,
            serde_json::json!({
                "name": "search_collection",
                "fields": [
                    {"name": "content", "type": "text"},
                    {"name": "label", "type": "text"},
                    {"name": "embedding", "type": "vector(1536)"},
                ],
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();

        rest::indexes::create(
            &cassie,
            "search_collection",
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "metric": "l2",
                }
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();

        let doc_one = rest::documents::create(
            &cassie,
            "search_collection",
            serde_json::json!({
                "content": "alpha",
                "label": "first",
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();
        let doc_two = rest::documents::create(
            &cassie,
            "search_collection",
            serde_json::json!({
                "content": "beta",
                "label": "second",
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();

        let first_id = doc_one["id"].as_str().expect("doc one id");
        let second_id = doc_two["id"].as_str().expect("doc two id");

        // Act
        let search = rest::search::vector_search(
            &cassie,
            "search_collection",
            serde_json::json!({
                "field": "embedding",
                "query": "query text",
                "metric": "l2",
                "limit": 2,
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();

        // Assert
        let rows = search["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 2);
        let returned_first_id = rows[0][0]
            .get("String")
            .and_then(serde_json::Value::as_str)
            .expect("first result id");
        let returned_second_id = rows[1][0]
            .get("String")
            .and_then(serde_json::Value::as_str)
            .expect("second result id");
        assert_eq!(returned_first_id, first_id);
        assert_eq!(returned_second_id, second_id);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_fail_vector_search_when_metric_incompatible_with_index() {
    // Arrange
    with_fallback();
    let path = data_dir("search_incompatible_metric");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .unwrap();

    let openai_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        openai_runtime_with_server(openai_server.base_url()),
    )
    .unwrap();

    runtime.block_on(async {
        cassie.startup().await.unwrap();

        rest::collections::create(
            &cassie,
            serde_json::json!({
                "name": "search_incompatible_collection",
                "fields": [
                    {"name": "content", "type": "text"},
                    {"name": "embedding", "type": "vector(1536)"},
                ],
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();

        rest::indexes::create(
            &cassie,
            "search_incompatible_collection",
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "metric": "cosine",
                }
            })
            .to_string()
            .as_bytes(),
        )
        .await
        .unwrap();

        // Act
        let result = rest::search::vector_search(
            &cassie,
            "search_incompatible_collection",
            serde_json::json!({
                "field": "embedding",
                "query": "query text",
                "metric": "l2",
            })
            .to_string()
            .as_bytes(),
        )
        .await;

        // Assert
        assert!(result.is_err());
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

fn read_http_body(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut headers_end = 0usize;
    let mut content_length = 0usize;
    while headers_end == 0 {
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            return Vec::new();
        }

        buffer.extend_from_slice(&chunk[..read]);
        if let Some(separator) = find_request_body_start(&buffer) {
            headers_end = separator;
            content_length = parse_content_length(&buffer);
        }
    }

    while buffer.len() < headers_end.saturating_add(content_length) {
        let read = stream.read(&mut chunk).expect("read request body");
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
    }

    buffer[headers_end..headers_end.saturating_add(content_length)].to_vec()
}

fn find_request_body_start(value: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(value);
    text.find("\r\n\r\n").map(|index| index + 4)
}

fn parse_content_length(value: &[u8]) -> usize {
    let header = String::from_utf8_lossy(value);
    for line in header.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            if let Ok(parsed) = value.trim().parse::<usize>() {
                return parsed;
            }
        }
    }
    0
}
