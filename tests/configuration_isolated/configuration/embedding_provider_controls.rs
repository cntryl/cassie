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

/// Serializes tests that run a provider call against a time-bounded local
/// server. Take it before starting the server so time spent waiting for
/// another test does not consume the server's accept window. A failed test
/// must not fail the others, so a poisoned guard is still usable.
fn controlled_request_guard() -> std::sync::MutexGuard<'static, ()> {
    CONTROLLED_REQUEST_WORKER_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn transient_server() -> (
    String,
    std::sync::mpsc::Receiver<Instant>,
    std::thread::JoinHandle<(usize, usize)>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind transient server");
    listener
        .set_nonblocking(true)
        .expect("configure transient server");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut request_count = 0usize;
        let mut response_count = 0usize;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(100)))
                        .expect("bound transient request read");
                    let mut request = [0_u8; 8_192];
                    if !matches!(stream.read(&mut request), Ok(bytes_read) if bytes_read > 0) {
                        continue;
                    }
                    request_count += 1;
                    let body = r#"{"error":"retry later"}"#;
                    let response = format!(
                        "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    if stream.write_all(response.as_bytes()).is_ok() {
                        response_count += 1;
                        let _ = response_tx.send(Instant::now());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("transient server accept failed: {error}"),
            }
        }
        (request_count, response_count)
    });
    (base_url, response_rx, thread)
}

fn assert_one_transient_request(server: std::thread::JoinHandle<(usize, usize)>) {
    assert_eq!(
        server.join().expect("transient server"),
        (1, 1),
        "the deadline probe should send one request and get one successful 429 write"
    );
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

fn deadline_limits() -> CassieRuntimeLimits {
    CassieRuntimeLimits {
        query_timeout_ms: 10,
        ..CassieRuntimeLimits::default()
    }
}

fn assert_deadline_interrupts_retry(
    provider: Box<dyn EmbeddingProvider>,
    response_written: &std::sync::mpsc::Receiver<Instant>,
) {
    // Shared runners can deschedule the test thread after the 10 ms deadline;
    // this still distinguishes deadline interruption from the 1 s transport timeout.
    const SCHEDULER_TOLERANT_LIMIT: Duration = Duration::from_millis(250);

    let limits = deadline_limits();
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let query_worker = std::thread::spawn(move || {
        let started = Instant::now();
        let controls =
            QueryExecutionControls::with_cancellation(&limits, started, query_cancellation);
        let deadline = controls.deadline.expect("configured query deadline");
        let result =
            provider.embed_documents_with_controls(&["bounded input".to_string()], &controls);
        let _ = result_tx.send((result, started, deadline));
    });
    let (error, started, deadline) = match result_rx.recv_timeout(SCHEDULER_TOLERANT_LIMIT) {
        Ok((result, started, deadline)) => (
            result.expect_err("deadline should interrupt provider retry"),
            started,
            deadline,
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            cancellation.cancel();
            let cleanup = result_rx.recv_timeout(SCHEDULER_TOLERANT_LIMIT);
            drop(query_worker);
            assert!(
                cleanup.is_ok(),
                "provider worker did not stop within the bounded cancellation cleanup"
            );
            panic!("provider call exceeded its bounded wait");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            drop(query_worker);
            panic!("provider worker exited without reporting its result");
        }
    };
    drop(query_worker);
    assert!(matches!(error, EmbeddingError::Timeout { .. }));
    assert!(
        started.elapsed() < SCHEDULER_TOLERANT_LIMIT,
        "provider retry exceeded the query deadline: {:?}",
        started.elapsed()
    );
    let response_written_at = response_written
        .recv_timeout(SCHEDULER_TOLERANT_LIMIT)
        .expect("server wrote the 429 response");
    assert!(
        response_written_at <= deadline,
        "the server wrote the 429 after the query deadline: {response_written_at:?} > {deadline:?}"
    );
}

#[test]
fn should_clamp_openai_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, response_written, server) = transient_server();
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
    assert_deadline_interrupts_retry(Box::new(provider), &response_written);

    // Assert
    assert_one_transient_request(server);
}

#[test]
fn should_clamp_openai_compatible_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, response_written, server) = transient_server();
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
    assert_deadline_interrupts_retry(Box::new(provider), &response_written);

    // Assert
    assert_one_transient_request(server);
}

#[test]
fn should_clamp_tei_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, response_written, server) = transient_server();
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
    assert_deadline_interrupts_retry(Box::new(provider), &response_written);

    // Assert
    assert_one_transient_request(server);
}

#[test]
fn should_clamp_ollama_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, response_written, server) = transient_server();
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
    assert_deadline_interrupts_retry(Box::new(provider), &response_written);

    // Assert
    assert_one_transient_request(server);
}

#[test]
fn should_clamp_voyage_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, response_written, server) = transient_server();
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
    assert_deadline_interrupts_retry(Box::new(provider), &response_written);

    // Assert
    assert_one_transient_request(server);
}

#[test]
fn should_clamp_cohere_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, response_written, server) = transient_server();
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
    assert_deadline_interrupts_retry(Box::new(provider), &response_written);

    // Assert
    assert_one_transient_request(server);
}

#[test]
fn should_cancel_an_active_provider_request_without_waiting_for_transport_timeout() {
    // Arrange
    let _guard = controlled_request_guard();
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
        .expect_err("active request should be canceled");

    // Assert
    assert!(matches!(error, EmbeddingError::Canceled { .. }));
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "cancellation waited for the provider transport: {:?}",
        started.elapsed()
    );
    assert_eq!(
        active_controlled_request_workers_for_diagnostics(),
        baseline_workers,
        "canceled request worker should terminate before the caller returns"
    );
    server.join().expect("delayed server");
}

#[test]
fn should_not_retry_mid_body_reset_for_any_remote_provider() {
    // Arrange
    let _guard = controlled_request_guard();
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
