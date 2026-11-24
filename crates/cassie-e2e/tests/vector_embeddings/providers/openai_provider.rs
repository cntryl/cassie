use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cassie::embeddings::openai::{OpenAiProvider, OpenAiProviderConfig};
use cassie::embeddings::DEFAULT_EMBEDDING_MODEL;
use cassie::embeddings::{EmbeddingError, EmbeddingProvider};

fn assert_f32_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= f32::EPSILON,
        "expected {actual} to equal {expected}"
    );
}

#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: String,
    delay_ms: u64,
}

struct MockOpenAiServer {
    base_url: String,
    observed_input_counts: Arc<Mutex<Vec<usize>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockOpenAiServer {
    fn spawn(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock openai server");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("listener local addr")
        );
        let base_url_clone = base_url.clone();
        let observed_input_counts = Arc::new(Mutex::new(Vec::new()));
        let observed = observed_input_counts.clone();

        let thread = thread::spawn(move || {
            let mut queue = VecDeque::from(responses);
            while let Some(response) = queue.pop_front() {
                let (mut stream, _) = listener.accept().expect("accept openai request");
                let body = read_http_request_body(&mut stream);
                let input_count = serde_json::from_slice::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("input")
                            .and_then(|value| value.as_array())
                            .map(std::vec::Vec::len)
                    })
                    .unwrap_or(0);

                observed.lock().expect("observed lock").push(input_count);

                if response.delay_ms > 0 {
                    thread::sleep(Duration::from_millis(response.delay_ms));
                }

                let reason = if response.status == 200 {
                    "OK"
                } else if response.status == 429 {
                    "Too Many Requests"
                } else if response.status == 500 {
                    "Internal Server Error"
                } else {
                    "Error"
                };

                let response_body = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status,
                    reason,
                    response.body.len(),
                    response.body
                );

                stream
                    .write_all(response_body.as_bytes())
                    .expect("write mock response");
                stream.flush().expect("flush mock response");
            }
        });

        Self {
            base_url: base_url_clone,
            observed_input_counts,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        self.base_url.clone()
    }

    fn observed_input_counts(&self) -> Vec<usize> {
        self.observed_input_counts
            .lock()
            .expect("observed lock")
            .clone()
    }
}

impl Drop for MockOpenAiServer {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn read_http_request_body(stream: &mut TcpStream) -> Vec<u8> {
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
        if let Some(separator) = find_http_header_terminator(&buffer) {
            headers_end = separator;
            content_length = parse_content_length(&buffer[..separator]);
        }
    }

    while buffer.len() < headers_end + content_length {
        let read = stream.read(&mut chunk).expect("read request body");
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
    }

    buffer[headers_end..headers_end + content_length].to_vec()
}

fn find_http_header_terminator(value: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(value);
    text.find("\r\n\r\n").map(|index| index + 4)
}

fn parse_content_length(value: &[u8]) -> usize {
    let header = String::from_utf8_lossy(value);
    for line in header.lines() {
        let lower = line.to_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            if let Ok(parsed) = value.trim().parse::<usize>() {
                return parsed;
            }
        }
    }

    0
}

fn response_body(vectors: &[Vec<f32>], include_usage: bool) -> String {
    let mut data = Vec::with_capacity(vectors.len());
    for (index, vector) in vectors.iter().enumerate() {
        data.push(serde_json::json!({"index": index, "embedding": vector}));
    }

    if include_usage {
        serde_json::json!({
            "data": data,
            "usage": {"prompt_tokens": 2, "total_tokens": 2},
        })
        .to_string()
    } else {
        serde_json::json!({"data": data}).to_string()
    }
}

fn constant_vector(value: f32) -> Vec<f32> {
    vec![value; 1536]
}

#[test]
fn should_build_requests_in_batches() {
    // Arrange
    let server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: response_body(&[constant_vector(0.1), constant_vector(0.2)], false),
            delay_ms: 0,
        },
        MockResponse {
            status: 200,
            body: response_body(&[constant_vector(0.3)], false),
            delay_ms: 0,
        },
    ]);

    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: DEFAULT_EMBEDDING_MODEL.to_string(),
        timeout: Duration::from_secs(1),
        max_batch_size: 2,
        max_retries: 1,
        base_url: server.base_url(),
    })
    .expect("openai provider");

    // Act
    let embeddings = provider
        .embed_documents(&[
            "first input".to_string(),
            "second input".to_string(),
            "third input".to_string(),
        ])
        .expect("embeddings should succeed");

    // Assert
    assert_eq!(embeddings.len(), 3);
    assert_eq!(server.observed_input_counts(), vec![2, 1]);
    assert_f32_close(embeddings[0].values[0], 0.1);
    assert_f32_close(embeddings[1].values[0], 0.2);
    assert_f32_close(embeddings[2].values[0], 0.3);
}

#[test]
fn should_retry_transient_failures() {
    // Arrange
    let server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 500,
            body: r#"{"error": "temporary"}"#.to_string(),
            delay_ms: 0,
        },
        MockResponse {
            status: 200,
            body: response_body(&[constant_vector(0.4)], false),
            delay_ms: 0,
        },
    ]);

    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: DEFAULT_EMBEDDING_MODEL.to_string(),
        timeout: Duration::from_secs(1),
        max_batch_size: 2,
        max_retries: 3,
        base_url: server.base_url(),
    })
    .expect("openai provider");

    // Act
    let embeddings = provider
        .embed_documents(&["retrying input".to_string()])
        .expect("embedding should recover after retry");

    // Assert
    assert_eq!(embeddings.len(), 1);
    assert_f32_close(embeddings[0].values[0], 0.4);
    assert_eq!(server.observed_input_counts(), vec![1, 1]);
}

#[test]
fn should_return_timeout_error() {
    // Arrange
    let server = MockOpenAiServer::spawn(vec![MockResponse {
        status: 200,
        body: response_body(&[vec![0.1, 0.2]], false),
        delay_ms: 150,
    }]);

    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: DEFAULT_EMBEDDING_MODEL.to_string(),
        timeout: Duration::from_millis(50),
        max_batch_size: 1,
        max_retries: 1,
        base_url: server.base_url(),
    })
    .expect("openai provider");

    // Act
    let result = provider.embed_documents(&["slow response".to_string()]);

    // Assert
    assert!(matches!(result, Err(EmbeddingError::Timeout { .. })));
}

#[test]
fn should_parse_openai_response_with_usage() {
    // Arrange
    let server = MockOpenAiServer::spawn(vec![MockResponse {
        status: 200,
        body: response_body(&[constant_vector(0.6)], true),
        delay_ms: 0,
    }]);

    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: DEFAULT_EMBEDDING_MODEL.to_string(),
        timeout: Duration::from_secs(1),
        max_batch_size: 2,
        max_retries: 1,
        base_url: server.base_url(),
    })
    .expect("openai provider");

    // Act
    let embeddings = provider
        .embed_documents(&["usage test".to_string()])
        .expect("embedding should parse response");

    // Assert
    assert_eq!(embeddings.len(), 1);
    assert_f32_close(embeddings[0].values[0], 0.6);
}

#[test]
fn should_return_embedding_with_expected_dimensions() {
    // Arrange
    let vector = constant_vector(0.9);
    let server = MockOpenAiServer::spawn(vec![MockResponse {
        status: 200,
        body: response_body(std::slice::from_ref(&vector), false),
        delay_ms: 0,
    }]);

    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: DEFAULT_EMBEDDING_MODEL.to_string(),
        timeout: Duration::from_secs(1),
        max_batch_size: 1,
        max_retries: 1,
        base_url: server.base_url(),
    })
    .expect("openai provider");

    // Act
    let embeddings = provider
        .embed_documents(&["length test".to_string()])
        .expect("embedding should parse");

    // Assert
    assert_eq!(embeddings[0].values.len(), 1536);
}
