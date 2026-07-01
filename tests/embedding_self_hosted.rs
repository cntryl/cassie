use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use cassie::embeddings::compatible::{OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig};
use cassie::embeddings::ollama::{OllamaProvider, OllamaProviderConfig};
use cassie::embeddings::tei::{TeiProvider, TeiProviderConfig};
use cassie::embeddings::EmbeddingProvider;

#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: String,
    expected_authorization: Option<String>,
}

impl MockResponse {
    fn ok(body: &serde_json::Value) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            expected_authorization: None,
        }
    }

    fn with_status(status: u16, body: &serde_json::Value) -> Self {
        Self {
            status,
            body: body.to_string(),
            expected_authorization: None,
        }
    }

    fn requiring_authorization(body: &serde_json::Value, authorization: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            expected_authorization: Some(authorization.to_string()),
        }
    }
}

struct MockEmbeddingServer {
    base_url: String,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockEmbeddingServer {
    fn spawn(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock embedding server");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("mock server address")
        );
        let thread = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("mock accept");
                let request = read_http_request(&mut stream);
                if let Some(expected) = &response.expected_authorization {
                    assert!(
                        request
                            .headers
                            .to_ascii_lowercase()
                            .contains(&format!("authorization: {}", expected.to_ascii_lowercase())),
                        "expected authorization header"
                    );
                }
                let body = request.body;
                if body.is_empty() {
                    continue;
                }

                let output = format!(
                    "HTTP/1.1 {} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    response.body
                );
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

impl Drop for MockEmbeddingServer {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            if std::thread::panicking() {
                drop(handle);
            } else {
                handle.join().expect("mock embedding server thread");
            }
        }
    }
}

#[test]
fn should_embed_documents_with_tei_provider() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!([
        [0.1, 0.2, 0.3],
        [0.4, 0.5, 0.6]
    ]))]);
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string(), "beta".to_string()];

    // Act
    let embeddings = provider
        .embed_documents(&inputs)
        .expect("embeddings should return");

    // Assert
    assert_eq!(provider.provider_name(), "tei");
    assert_eq!(provider.model_name(), "BAAI/bge-small-en-v1.5");
    assert_eq!(provider.dimensions(), 3);
    assert_eq!(embeddings.len(), 2);
    assert_eq!(embeddings[0].values, vec![0.1, 0.2, 0.3]);
}

#[test]
fn should_embed_documents_with_openai_compatible_provider() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!({
            "data": [
                {"index": 1, "embedding": [0.4, 0.5, 0.6]},
                {"index": 0, "embedding": [0.1, 0.2, 0.3]}
            ]
    }))]);
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        base_url: server.base_url(),
        api_key: None,
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string(), "beta".to_string()];

    // Act
    let embeddings = provider
        .embed_documents(&inputs)
        .expect("embeddings should return");

    // Assert
    assert_eq!(provider.provider_name(), "openai_compatible");
    assert_eq!(provider.model_name(), "BAAI/bge-small-en-v1.5");
    assert_eq!(provider.dimensions(), 3);
    assert_eq!(embeddings[0].values, vec![0.1, 0.2, 0.3]);
    assert_eq!(embeddings[1].values, vec![0.4, 0.5, 0.6]);
}

#[test]
fn should_send_openai_compatible_authorization_header() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![MockResponse::requiring_authorization(
        &serde_json::json!({
            "data": [
                {"index": 0, "embedding": [0.1, 0.2, 0.3]}
            ]
        }),
        "Bearer secret-token",
    )]);
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        base_url: server.base_url(),
        api_key: Some("secret-token".to_string()),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string()];

    // Act
    let embeddings = provider
        .embed_documents(&inputs)
        .expect("embeddings should return");

    // Assert
    assert_eq!(embeddings.len(), 1);
}

#[test]
fn should_embed_documents_inside_current_thread_runtime() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!([[
        0.1, 0.2, 0.3
    ]]))]);
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string()];

    // Act
    let embeddings = provider.embed_documents(&inputs);

    // Assert
    assert_eq!(
        embeddings.expect("embeddings should return")[0].values,
        vec![0.1, 0.2, 0.3]
    );
}

#[test]
fn should_embed_documents_with_ollama_provider() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!({
            "model": "nomic-embed-text",
            "embeddings": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]
    }))]);
    let provider = OllamaProvider::with_config(OllamaProviderConfig {
        base_url: server.base_url(),
        model: "nomic-embed-text".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string(), "beta".to_string()];

    // Act
    let embeddings = provider
        .embed_documents(&inputs)
        .expect("embeddings should return");

    // Assert
    assert_eq!(provider.provider_name(), "ollama");
    assert_eq!(provider.model_name(), "nomic-embed-text");
    assert_eq!(provider.dimensions(), 3);
    assert_eq!(embeddings.len(), 2);
    assert_eq!(embeddings[1].values, vec![0.4, 0.5, 0.6]);
}

#[test]
fn should_reject_self_hosted_embedding_dimension_mismatch() {
    // Arrange
    let server =
        MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!([[0.1, 0.2]]))]);
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string()];

    // Act
    let result = provider.embed_documents(&inputs);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_self_hosted_embedding_response_count_mismatch() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!([[
        0.1, 0.2, 0.3
    ]]))]);
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 1,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string(), "beta".to_string()];

    // Act
    let result = provider.embed_documents(&inputs);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_retry_transient_self_hosted_embedding_failures() {
    // Arrange
    let server = MockEmbeddingServer::spawn(vec![
        MockResponse::with_status(503, &serde_json::json!({"error":"not ready"})),
        MockResponse::ok(&serde_json::json!([[0.1, 0.2, 0.3]])),
    ]);
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout: std::time::Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 2,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string()];

    // Act
    let embeddings = provider
        .embed_documents(&inputs)
        .expect("retry should succeed");

    // Assert
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].values, vec![0.1, 0.2, 0.3]);
}

struct HttpRequest {
    headers: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut std::net::TcpStream) -> HttpRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut headers_end = 0usize;
    let mut content_length = 0usize;
    while headers_end == 0 {
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            return HttpRequest {
                headers: String::new(),
                body: Vec::new(),
            };
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

    HttpRequest {
        headers: String::from_utf8_lossy(&buffer[..headers_end]).to_string(),
        body: buffer[headers_end..headers_end.saturating_add(content_length)].to_vec(),
    }
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
