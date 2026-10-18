use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::{fmt::Write as FmtWrite, path::Path};

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig,
    SelfHostedEmbeddingRuntimeConfig,
};
use cassie::embeddings::openai::OpenAiConfig;
use cassie::embeddings::DEFAULT_EMBEDDING_MODEL;
use cassie::midge::adapter::StorageFamily;
use cassie::rest;
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

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
                let _ = write!(output, "HTTP/1.1 {} OK\r\n", response.status);
                output.push_str("content-type: application/json\r\n");
                let _ = write!(output, "content-length: {}\r\n", response.body.len());
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

fn use_local_storage() {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
}

fn canonical_collection(name: &str) -> String {
    canonical_relation_name("postgres", "public", name)
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
    let collection = canonical_collection(collection);
    let prefix = cassie
        .midge
        .normalized_vector_prefix_for_diagnostics(&collection, field)
        .unwrap();
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    for (key, _) in entries {
        tx.delete(key).unwrap();
    }
    tx.commit(WriteOptions::sync()).unwrap();
}

fn create_vector_collection(cassie: &Cassie, collection: &str, dimensions: usize) {
    rest::collections::create(
        cassie,
        serde_json::json!({
            "name": collection,
            "fields": [
                {"name": "content", "type": "text"},
                {"name": "label", "type": "text"},
                {"name": "embedding", "type": format!("vector({dimensions})")},
            ],
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
}

fn create_vector_index(cassie: &Cassie, collection: &str, metric: &str) {
    rest::indexes::create(
        cassie,
        collection,
        serde_json::json!({
            "kind": "vector",
            "field": "embedding",
            "options": {
                "source_field": "content",
                "metric": metric,
            }
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
}

fn create_vector_index_with_options(
    cassie: &Cassie,
    collection: &str,
    options: &serde_json::Value,
) -> serde_json::Value {
    rest::indexes::create(
        cassie,
        collection,
        serde_json::json!({
            "kind": "vector",
            "field": "embedding",
            "options": options,
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap()
}

fn create_labelled_document(
    cassie: &Cassie,
    collection: &str,
    content: &str,
    label: &str,
) -> String {
    rest::documents::create(
        cassie,
        collection,
        serde_json::json!({"content": content, "label": label})
            .to_string()
            .as_bytes(),
    )
    .unwrap()["id"]
        .as_str()
        .expect("document id")
        .to_string()
}

fn vector_search(
    cassie: &Cassie,
    collection: &str,
    metric: &str,
    limit: usize,
    offset: usize,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "field": "embedding",
        "query": "query text",
        "metric": metric,
        "limit": limit,
    });
    if offset > 0 {
        body["offset"] = serde_json::json!(offset);
    }
    serde_json::to_value(
        rest::search::vector_search(cassie, collection, body.to_string().as_bytes()).unwrap(),
    )
    .expect("search response json")
}

fn row_ids(search: &serde_json::Value) -> Vec<String> {
    search["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|row| row[0].as_str().expect("row id").to_string())
        .collect()
}

fn cleanup_path(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn assert_normalized_fallback_metrics(
    before: &serde_json::Value,
    after_normalized: &serde_json::Value,
    after_fallback: &serde_json::Value,
) {
    let before_normalized = before["vector"]["normalized_candidate_count_total"]
        .as_u64()
        .unwrap_or_default();
    let before_fallback = before["vector"]["normalized_fallback_count_total"]
        .as_u64()
        .unwrap_or_default();
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

    let search = serde_json::to_value(search).expect("search response json");
    let rows = search["rows"].as_array().expect("rows array");
    let returned = rows
        .iter()
        .map(|row| row[0].as_str().expect("result id").to_string())
        .collect::<Vec<_>>();
    assert_eq!(returned, vec![first_id, second_id]);
    returned
}

include!("semantics.rs");

include!("errors.rs");

include!("metadata.rs");

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
