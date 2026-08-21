use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cassie::config::CassieRuntimeLimits;
use cassie::embeddings::cohere::{CohereProvider, CohereProviderConfig};
use cassie::embeddings::compatible::{OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig};
use cassie::embeddings::ollama::{OllamaProvider, OllamaProviderConfig};
use cassie::embeddings::openai::{OpenAiProvider, OpenAiProviderConfig};
use cassie::embeddings::provider::active_controlled_request_workers_for_diagnostics;
use cassie::embeddings::tei::{TeiProvider, TeiProviderConfig};
use cassie::embeddings::voyage::{VoyageProvider, VoyageProviderConfig};
use cassie::embeddings::{EmbeddingError, EmbeddingProvider};
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};

static CONTROLLED_REQUEST_WORKER_GUARD: Mutex<()> = Mutex::new(());

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

fn mid_body_reset_server() -> (String, std::thread::JoinHandle<usize>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind reset server");
    listener
        .set_nonblocking(true)
        .expect("configure reset server");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut request_count = 0usize;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    request_count += 1;
                    let mut request = [0_u8; 8_192];
                    let _ = stream.read(&mut request);
                    let response = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 64\r\nconnection: close\r\n\r\n{\"partial\":";
                    let _ = stream.write_all(response);
                    let _ = stream.flush();
                    let _ = stream.shutdown(Shutdown::Both);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("reset server accept failed: {error}"),
            }
        }
        request_count
    });
    (base_url, thread)
}

fn assert_mid_body_reset_is_not_retried(
    provider_factory: impl FnOnce(String) -> Box<dyn EmbeddingProvider>,
) {
    let (base_url, server) = mid_body_reset_server();
    let provider = provider_factory(base_url);
    let error = provider
        .embed_documents(&["retry consistency".to_string()])
        .expect_err("truncated provider response should fail");
    assert!(matches!(error, EmbeddingError::RequestError(_)));
    assert_eq!(server.join().expect("reset server"), 1);
}

fn deadline_controls() -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_timeout_ms: 10,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

fn assert_deadline_interrupts_retry(provider: &dyn EmbeddingProvider) {
    // Shared runners can deschedule the test thread after the 10 ms deadline;
    // this still distinguishes deadline interruption from the 1 s transport timeout.
    const SCHEDULER_TOLERANT_LIMIT: Duration = Duration::from_millis(250);

    let _guard = CONTROLLED_REQUEST_WORKER_GUARD
        .lock()
        .expect("lock controlled request worker guard");
    let controls = deadline_controls();
    let started = Instant::now();
    let error = provider
        .embed_documents_with_controls(&["bounded input".to_string()], &controls)
        .expect_err("deadline should interrupt provider retry");
    assert!(matches!(error, EmbeddingError::Timeout { .. }));
    assert!(
        started.elapsed() < SCHEDULER_TOLERANT_LIMIT,
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
    let _guard = CONTROLLED_REQUEST_WORKER_GUARD
        .lock()
        .expect("lock controlled request worker guard");
    let baseline_workers = active_controlled_request_workers_for_diagnostics();
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
    assert_eq!(
        active_controlled_request_workers_for_diagnostics(),
        baseline_workers,
        "cancelled request worker should terminate before the caller returns"
    );
    server.join().expect("delayed server");
}

#[test]
fn should_not_retry_mid_body_reset_for_any_remote_provider() {
    // Arrange
    let _guard = CONTROLLED_REQUEST_WORKER_GUARD
        .lock()
        .expect("lock controlled request worker guard");
    let baseline_workers = active_controlled_request_workers_for_diagnostics();

    // Act
    assert_mid_body_reset_is_not_retried(|base_url| {
        Box::new(
            OpenAiProvider::with_config(OpenAiProviderConfig {
                api_key: "test-key".to_string(),
                model: "text-embedding-3-small".to_string(),
                timeout: Duration::from_secs(1),
                max_batch_size: 8,
                max_retries: 3,
                base_url,
            })
            .expect("OpenAI provider"),
        )
    });
    assert_mid_body_reset_is_not_retried(|base_url| {
        Box::new(
            OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
                base_url,
                api_key: Some("test-key".to_string()),
                model: "compatible-test".to_string(),
                dimensions: 3,
                timeout: Duration::from_secs(1),
                max_batch_size: 8,
                max_retries: 3,
            })
            .expect("compatible provider"),
        )
    });
    assert_mid_body_reset_is_not_retried(|base_url| {
        Box::new(
            TeiProvider::with_config(TeiProviderConfig {
                base_url,
                model: "tei-test".to_string(),
                dimensions: 3,
                timeout: Duration::from_secs(1),
                max_batch_size: 8,
                max_retries: 3,
            })
            .expect("TEI provider"),
        )
    });
    assert_mid_body_reset_is_not_retried(|base_url| {
        Box::new(
            OllamaProvider::with_config(OllamaProviderConfig {
                base_url,
                model: "ollama-test".to_string(),
                dimensions: 3,
                timeout: Duration::from_secs(1),
                max_batch_size: 8,
                max_retries: 3,
            })
            .expect("Ollama provider"),
        )
    });
    assert_mid_body_reset_is_not_retried(|base_url| {
        Box::new(
            VoyageProvider::with_config(VoyageProviderConfig {
                api_key: "test-key".to_string(),
                model: "voyage-test".to_string(),
                dimensions: 3,
                timeout: Duration::from_secs(1),
                max_batch_size: 8,
                max_retries: 3,
                base_url,
            })
            .expect("Voyage provider"),
        )
    });
    assert_mid_body_reset_is_not_retried(|base_url| {
        Box::new(
            CohereProvider::with_config(CohereProviderConfig {
                api_key: "test-key".to_string(),
                model: "cohere-test".to_string(),
                dimensions: 3,
                timeout: Duration::from_secs(1),
                max_batch_size: 8,
                max_retries: 3,
                base_url,
            })
            .expect("Cohere provider"),
        )
    });

    // Assert
    assert_eq!(
        active_controlled_request_workers_for_diagnostics(),
        baseline_workers
    );
}
