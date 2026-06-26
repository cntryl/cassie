use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig,
    SelfHostedEmbeddingRuntimeConfig,
};
use cassie::embeddings::openai::OpenAiConfig;
use cassie::embeddings::{NormalizedVectorRecord, DEFAULT_EMBEDDING_MODEL};
use cassie::midge::adapter::StorageFamily;
use cassie::rest;
use cntryl_midge::{TransactionMode, WriteOptions};
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
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
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

fn tei_runtime_with_server(base_url: String) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
        base_url,
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout_seconds: 2,
        max_batch_size: 3,
        max_retries: 1,
    });
    config
}

fn ollama_runtime_with_server(base_url: String) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Ollama(SelfHostedEmbeddingRuntimeConfig {
        base_url,
        model: "nomic-embed-text".to_string(),
        dimensions: 3,
        timeout_seconds: 2,
        max_batch_size: 3,
        max_retries: 1,
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

fn tei_response_body(vectors: &[Vec<f32>]) -> String {
    serde_json::to_string(vectors).expect("tei response")
}

fn ollama_response_body(vectors: &[Vec<f32>]) -> String {
    serde_json::json!({
        "model": "nomic-embed-text",
        "embeddings": vectors,
    })
    .to_string()
}

fn clear_normalized_sidecars(cassie: &Cassie, collection: &str, field: &str) {
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    for (key, value) in entries {
        let Ok(record) = serde_json::from_slice::<NormalizedVectorRecord>(&value) else {
            continue;
        };
        if record.collection == collection && record.field == field {
            tx.delete(key).unwrap();
        }
    }
    tx.commit(WriteOptions::sync()).unwrap();
}

fn search_self_hosted_vector_docs(cassie: &Cassie, collection: &str) -> Vec<String> {
    rest::collections::create(
        cassie,
        serde_json::json!({
            "name": collection,
            "fields": [
                {"name": "content", "type": "text"},
                {"name": "label", "type": "text"},
                {"name": "embedding", "type": "vector(3)"},
            ],
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    rest::indexes::create(
        cassie,
        collection,
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
    .unwrap();

    let doc_one = rest::documents::create(
        cassie,
        collection,
        serde_json::json!({
            "content": "alpha",
            "label": "first",
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let doc_two = rest::documents::create(
        cassie,
        collection,
        serde_json::json!({
            "content": "beta",
            "label": "second",
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    let first_id = doc_one["id"].as_str().expect("doc one id").to_string();
    let second_id = doc_two["id"].as_str().expect("doc two id").to_string();

    let search = rest::search::vector_search(
        cassie,
        collection,
        serde_json::json!({
            "field": "embedding",
            "query": "query text",
            "metric": "l2",
            "limit": 2,
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    let rows = search["rows"].as_array().expect("rows array");
    let returned = rows
        .iter()
        .map(|row| {
            row[0]
                .get("String")
                .and_then(serde_json::Value::as_str)
                .expect("result id")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(returned, vec![first_id, second_id]);
    returned
}

#[test]
fn should_search_vector_docs_after_ingest() {
    // Arrange
    with_fallback();
    let path = data_dir("search_flow");
    let path_for_cleanup = path.clone();

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

    // Arrange
    cassie.startup().unwrap();

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

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_search_vector_docs_with_tei_provider() {
    // Arrange
    with_fallback();
    let path = data_dir("tei_search_flow");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![5.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 0.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

    // Act
    let rows = search_self_hosted_vector_docs(&cassie, "tei_search_collection");
    // Assert
    assert_eq!(rows.len(), 2);

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_apply_vector_search_offset_after_distance_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_search_offset");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![5.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![3.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 0.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();
    rest::collections::create(
        &cassie,
        serde_json::json!({
            "name": "vector_offset_collection",
            "fields": [
                {"name": "content", "type": "text"},
                {"name": "label", "type": "text"},
                {"name": "embedding", "type": "vector(3)"},
            ],
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    rest::indexes::create(
        &cassie,
        "vector_offset_collection",
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
    .unwrap();

    rest::documents::create(
        &cassie,
        "vector_offset_collection",
        serde_json::json!({"content": "far", "label": "third"})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let nearest = rest::documents::create(
        &cassie,
        "vector_offset_collection",
        serde_json::json!({"content": "near", "label": "first"})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let middle = rest::documents::create(
        &cassie,
        "vector_offset_collection",
        serde_json::json!({"content": "middle", "label": "second"})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let nearest_id = nearest["id"].as_str().expect("nearest id");
    let middle_id = middle["id"].as_str().expect("middle id");

    // Act
    let search = rest::search::vector_search(
        &cassie,
        "vector_offset_collection",
        serde_json::json!({
            "field": "embedding",
            "query": "query text",
            "metric": "l2",
            "limit": 1,
            "offset": 1,
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    // Assert
    let rows = search["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 1);
    let returned_id = rows[0][0]
        .get("String")
        .and_then(serde_json::Value::as_str)
        .expect("result id");
    assert_ne!(returned_id, nearest_id);
    assert_eq!(returned_id, middle_id);

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_fall_back_to_raw_vector_search_when_normalized_sidecars_are_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_search_normalized_fallback");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![3.0, 4.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 5.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![3.0, 4.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

    rest::collections::create(
        &cassie,
        serde_json::json!({
            "name": "vector_search_normalized_fallback",
            "fields": [
                {"name": "content", "type": "text"},
                {"name": "label", "type": "text"},
                {"name": "embedding", "type": "vector(3)"},
            ],
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    rest::indexes::create(
        &cassie,
        "vector_search_normalized_fallback",
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
    .unwrap();

    let first = rest::documents::create(
        &cassie,
        "vector_search_normalized_fallback",
        serde_json::json!({"content": "alpha", "label": "first"})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let second = rest::documents::create(
        &cassie,
        "vector_search_normalized_fallback",
        serde_json::json!({"content": "beta", "label": "second"})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let first_id = first["id"].as_str().expect("first id").to_string();
    let second_id = second["id"].as_str().expect("second id").to_string();

    let before = cassie.metrics();
    let before_normalized = before["vector"]["normalized_candidate_count_total"]
        .as_u64()
        .unwrap_or_default();
    let before_fallback = before["vector"]["normalized_fallback_count_total"]
        .as_u64()
        .unwrap_or_default();

    // Act
    let normalized_search = rest::search::vector_search(
        &cassie,
        "vector_search_normalized_fallback",
        serde_json::json!({
            "field": "embedding",
            "query": "query text",
            "metric": "cosine",
            "limit": 2,
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let after_normalized = cassie.metrics();

    clear_normalized_sidecars(&cassie, "vector_search_normalized_fallback", "embedding");

    let fallback_search = rest::search::vector_search(
        &cassie,
        "vector_search_normalized_fallback",
        serde_json::json!({
            "field": "embedding",
            "query": "query text",
            "metric": "cosine",
            "limit": 2,
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let after_fallback = cassie.metrics();

    // Assert
    assert_eq!(normalized_search, fallback_search);
    let rows = normalized_search["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 2);
    let returned_ids = rows
        .iter()
        .map(|row| {
            row[0]
                .get("String")
                .and_then(serde_json::Value::as_str)
                .expect("row id")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(returned_ids, vec![first_id, second_id]);

    assert_eq!(
        after_normalized["vector"]["normalized_candidate_count_total"]
            .as_u64()
            .unwrap_or_default()
            - before_normalized,
        2
    );
    assert_eq!(
        after_normalized["vector"]["normalized_fallback_count_total"]
            .as_u64()
            .unwrap_or_default()
            - before_fallback,
        0
    );
    assert_eq!(
        after_fallback["vector"]["normalized_candidate_count_total"]
            .as_u64()
            .unwrap_or_default()
            - after_normalized["vector"]["normalized_candidate_count_total"]
                .as_u64()
                .unwrap_or_default(),
        0
    );
    assert_eq!(
        after_fallback["vector"]["normalized_fallback_count_total"]
            .as_u64()
            .unwrap_or_default()
            - after_normalized["vector"]["normalized_fallback_count_total"]
                .as_u64()
                .unwrap_or_default(),
        2
    );

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_search_vector_docs_with_ollama_provider() {
    // Arrange
    with_fallback();
    let path = data_dir("ollama_search_flow");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: ollama_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: ollama_response_body(&[vec![5.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: ollama_response_body(&[vec![0.0, 0.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        ollama_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

    // Act
    let rows = search_self_hosted_vector_docs(&cassie, "ollama_search_collection");
    // Assert
    assert_eq!(rows.len(), 2);

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_fail_vector_search_when_metric_incompatible_with_index() {
    // Arrange
    with_fallback();
    let path = data_dir("search_incompatible_metric");
    let path_for_cleanup = path.clone();

    let openai_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        openai_runtime_with_server(openai_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

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
    );

    // Assert
    assert!(result.is_err());

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
