use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use cassie::config::CassieRuntimeLimits;
use cassie::embeddings::cohere::{CohereProvider, CohereProviderConfig};
use cassie::embeddings::compatible::{OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig};
use cassie::embeddings::ollama::{OllamaProvider, OllamaProviderConfig};
use cassie::embeddings::openai::{OpenAiProvider, OpenAiProviderConfig};
use cassie::embeddings::tei::{TeiProvider, TeiProviderConfig};
use cassie::embeddings::voyage::{VoyageProvider, VoyageProviderConfig};
use cassie::embeddings::{EmbeddingError, EmbeddingProvider};
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn transient_server() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind transient server");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept provider request");
        let mut request = [0_u8; 8_192];
        let _ = stream.read(&mut request);
        let body = r#"{"error":"retry later"}"#;
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write transient response");
    });
    (base_url, thread)
}

fn delayed_tei_server() -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind delayed server");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let (accepted_tx, accepted_rx) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept provider request");
        let mut request = [0_u8; 8_192];
        let _ = stream.read(&mut request);
        accepted_tx.send(()).expect("signal accepted request");
        std::thread::sleep(Duration::from_millis(150));
        let body = r"[[0.1,0.2,0.3]]";
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (base_url, accepted_rx, thread)
}

fn deadline_controls() -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_timeout_ms: 10,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

fn assert_deadline_interrupts_retry(provider: &dyn EmbeddingProvider) {
    let controls = deadline_controls();
    let started = Instant::now();
    let error = provider
        .embed_documents_with_controls(&["bounded input".to_string()], &controls)
        .expect_err("deadline should interrupt provider retry");
    assert!(matches!(error, EmbeddingError::Timeout { .. }));
    assert!(
        started.elapsed() < Duration::from_millis(45),
        "provider retry exceeded the query deadline: {:?}",
        started.elapsed()
    );
}

#[test]
fn should_clamp_openai_retry_backoff_to_query_deadline() {
    // Arrange
    let (base_url, server) = transient_server();
    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: "text-embedding-3-small".to_string(),
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 3,
        base_url,
    })
    .expect("configure OpenAI provider");

    // Act
    assert_deadline_interrupts_retry(&provider);

    // Assert
    server.join().expect("transient server");
}

#[test]
fn should_clamp_openai_compatible_retry_backoff_to_query_deadline() {
    // Arrange
    let (base_url, server) = transient_server();
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        base_url,
        api_key: Some("test-key".to_string()),
        model: "compatible-test".to_string(),
        dimensions: 3,
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 3,
    })
    .expect("configure compatible provider");

    // Act
    assert_deadline_interrupts_retry(&provider);

    // Assert
    server.join().expect("transient server");
}

#[test]
fn should_clamp_tei_retry_backoff_to_query_deadline() {
    // Arrange
    let (base_url, server) = transient_server();
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url,
        model: "tei-test".to_string(),
        dimensions: 3,
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 3,
    })
    .expect("configure TEI provider");

    // Act
    assert_deadline_interrupts_retry(&provider);

    // Assert
    server.join().expect("transient server");
}

#[test]
fn should_clamp_ollama_retry_backoff_to_query_deadline() {
    // Arrange
    let (base_url, server) = transient_server();
    let provider = OllamaProvider::with_config(OllamaProviderConfig {
        base_url,
        model: "ollama-test".to_string(),
        dimensions: 3,
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 3,
    })
    .expect("configure Ollama provider");

    // Act
    assert_deadline_interrupts_retry(&provider);

    // Assert
    server.join().expect("transient server");
}

#[test]
fn should_clamp_voyage_retry_backoff_to_query_deadline() {
    // Arrange
    let (base_url, server) = transient_server();
    let provider = VoyageProvider::with_config(VoyageProviderConfig {
        api_key: "test-key".to_string(),
        model: "voyage-test".to_string(),
        dimensions: 3,
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 3,
        base_url,
    })
    .expect("configure Voyage provider");

    // Act
    assert_deadline_interrupts_retry(&provider);

    // Assert
    server.join().expect("transient server");
}

#[test]
fn should_clamp_cohere_retry_backoff_to_query_deadline() {
    // Arrange
    let (base_url, server) = transient_server();
    let provider = CohereProvider::with_config(CohereProviderConfig {
        api_key: "test-key".to_string(),
        model: "cohere-test".to_string(),
        dimensions: 3,
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 3,
        base_url,
    })
    .expect("configure Cohere provider");

    // Act
    assert_deadline_interrupts_retry(&provider);

    // Assert
    server.join().expect("transient server");
}

#[test]
fn should_cancel_an_active_provider_request_without_waiting_for_transport_timeout() {
    // Arrange
    let (base_url, accepted, server) = delayed_tei_server();
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url,
        model: "tei-test".to_string(),
        dimensions: 3,
        timeout: Duration::from_secs(1),
        max_batch_size: 8,
        max_retries: 0,
    })
    .expect("configure TEI provider");
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query = std::thread::spawn(move || {
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            query_cancellation,
        );
        provider.embed_documents_with_controls(&["bounded input".to_string()], &controls)
    });
    accepted.recv().expect("provider request accepted");
    let started = Instant::now();

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("provider thread")
        .expect_err("active request should be cancelled");

    // Assert
    assert!(matches!(error, EmbeddingError::Cancelled { .. }));
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "cancellation waited for the provider transport: {:?}",
        started.elapsed()
    );
    server.join().expect("delayed server");
}
