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

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-rest-embeds-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
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

    cassie.startup().unwrap();
    create_vector_collection(&cassie, "search_collection", 1536);
    create_vector_index(&cassie, "search_collection", "l2");
    let first_id = create_labelled_document(&cassie, "search_collection", "alpha", "first");
    let second_id = create_labelled_document(&cassie, "search_collection", "beta", "second");

    // Act
    let search = vector_search(&cassie, "search_collection", "l2", 2, 0);

    // Assert
    assert_eq!(row_ids(&search), vec![first_id, second_id]);

    cleanup_path(Path::new(&path_for_cleanup));
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
    create_vector_collection(&cassie, "vector_offset_collection", 3);
    create_vector_index(&cassie, "vector_offset_collection", "l2");
    let _ = create_labelled_document(&cassie, "vector_offset_collection", "far", "third");
    let nearest_id = create_labelled_document(&cassie, "vector_offset_collection", "near", "first");
    let middle_id =
        create_labelled_document(&cassie, "vector_offset_collection", "middle", "second");

    // Act
    let search = vector_search(&cassie, "vector_offset_collection", "l2", 1, 1);

    // Assert
    let returned_id = row_ids(&search).into_iter().next().expect("offset row");
    assert_ne!(returned_id, nearest_id);
    assert_eq!(returned_id, middle_id);

    cleanup_path(Path::new(&path_for_cleanup));
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
    create_vector_collection(&cassie, "vector_search_normalized_fallback", 3);
    create_vector_index(&cassie, "vector_search_normalized_fallback", "cosine");
    let first_id = create_labelled_document(
        &cassie,
        "vector_search_normalized_fallback",
        "alpha",
        "first",
    );
    let second_id = create_labelled_document(
        &cassie,
        "vector_search_normalized_fallback",
        "beta",
        "second",
    );

    let before = cassie.metrics();

    // Act
    let normalized_search =
        vector_search(&cassie, "vector_search_normalized_fallback", "cosine", 2, 0);
    let after_normalized = cassie.metrics();

    clear_normalized_sidecars(&cassie, "vector_search_normalized_fallback", "embedding");

    let fallback_search =
        vector_search(&cassie, "vector_search_normalized_fallback", "cosine", 2, 0);
    let after_fallback = cassie.metrics();

    // Assert
    assert_eq!(normalized_search, fallback_search);
    assert_eq!(row_ids(&normalized_search), vec![first_id, second_id]);
    assert_normalized_fallback_metrics(&before, &after_normalized, &after_fallback);

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_create_hnsw_vector_index_with_rest_option_parity() {
    // Arrange
    with_fallback();
    let path = data_dir("rest_hnsw_options");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_hnsw_options", 3);

    // Act
    let created = create_vector_index_with_options(
        &cassie,
        "rest_hnsw_options",
        &serde_json::json!({
            "source_field": "content",
            "metric": "l2",
            "index_type": "hnsw",
            "m": "12",
            "ef_construction": "96",
            "ef_search": "48",
        }),
    );
    let stored = cassie
        .midge
        .get_vector_index(&canonical_collection("rest_hnsw_options"), "embedding")
        .unwrap()
        .expect("stored vector index");

    // Assert
    assert_eq!(created["index_type"], "hnsw");
    assert_eq!(stored.metadata.index_type.as_str(), "hnsw");
    let hnsw = stored.metadata.hnsw.expect("hnsw options");
    assert_eq!(hnsw.m, 12);
    assert_eq!(hnsw.ef_construction, 96);
    assert_eq!(hnsw.ef_search, 48);

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_reject_invalid_rest_vector_index_options() {
    // Arrange
    with_fallback();
    let path = data_dir("rest_invalid_hnsw_options");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_invalid_hnsw_options", 3);

    // Act
    let error = rest::indexes::create(
        &cassie,
        "rest_invalid_hnsw_options",
        serde_json::json!({
            "kind": "vector",
            "field": "embedding",
            "options": {
                "source_field": "content",
                "index_type": "hnsw",
                "m": "1",
            }
        })
        .to_string()
        .as_bytes(),
    )
    .expect_err("invalid hnsw m should fail");

    // Assert
    assert!(error
        .to_string()
        .contains("vector index option 'm' must be in [2, 128]"));

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_search_rest_hnsw_vector_index_with_graph_execution() {
    // Arrange
    with_fallback();
    let path = data_dir("rest_hnsw_search");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 1.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
    ]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_hnsw_search", 3);
    create_vector_index_with_options(
        &cassie,
        "rest_hnsw_search",
        &serde_json::json!({
            "source_field": "content",
            "metric": "l2",
            "index_type": "hnsw",
            "m": "2",
            "ef_construction": "4",
            "ef_search": "2",
        }),
    );
    let nearest = create_labelled_document(&cassie, "rest_hnsw_search", "near", "first");
    let _ = create_labelled_document(&cassie, "rest_hnsw_search", "far", "second");
    let before = cassie.metrics();

    // Act
    let search = vector_search(&cassie, "rest_hnsw_search", "l2", 1, 0);
    let after = cassie.metrics();

    // Assert
    assert_eq!(row_ids(&search), vec![nearest]);
    assert_eq!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            - before["vector"]["hnsw_executions"].as_u64().unwrap(),
        1
    );

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_search_rest_ivfflat_vector_index_with_trained_candidates() {
    // Arrange
    with_fallback();
    let path = data_dir("rest_ivfflat_search");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 1.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
    ]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_ivfflat_search", 3);
    let created = create_vector_index_with_options(
        &cassie,
        "rest_ivfflat_search",
        &serde_json::json!({
            "source_field": "content",
            "metric": "l2",
            "index_type": "ivfflat",
            "lists": "2",
            "probes": "1",
            "training_sample_size": "2",
            "training_seed": "7",
        }),
    );
    let nearest = create_labelled_document(&cassie, "rest_ivfflat_search", "near", "first");
    let _ = create_labelled_document(&cassie, "rest_ivfflat_search", "far", "second");
    let before = cassie.metrics();

    // Act
    let search = vector_search(&cassie, "rest_ivfflat_search", "l2", 1, 0);
    let after = cassie.metrics();

    // Assert
    assert_eq!(created["index_type"], "ivfflat");
    assert_eq!(row_ids(&search), vec![nearest]);
    assert_eq!(
        after["vector"]["ivfflat_executions"].as_u64().unwrap()
            - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
        1
    );

    cleanup_path(Path::new(&path_for_cleanup));
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
