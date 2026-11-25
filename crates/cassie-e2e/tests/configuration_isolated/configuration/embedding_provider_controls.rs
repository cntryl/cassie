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

/// A local server that answers every request with a transient 429 until it is
/// stopped, so a provider keeps retrying until something else interrupts it.
struct TransientServer {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<usize>,
}

impl TransientServer {
    /// Stops the server and returns how many 429 responses it wrote.
    fn stop(self) -> usize {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.join().expect("transient server")
    }
}

fn transient_server() -> (String, TransientServer) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind transient server");
    listener
        .set_nonblocking(true)
        .expect("configure transient server");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = std::sync::Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let mut response_count = 0usize;
        while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("blocking transient stream");
                    stream
                        .set_read_timeout(Some(Duration::from_millis(500)))
                        .expect("bound transient request read");
                    let mut request = [0_u8; 8_192];
                    if !matches!(stream.read(&mut request), Ok(bytes_read) if bytes_read > 0) {
                        continue;
                    }
                    let body = r#"{"error":"retry later"}"#;
                    let response = format!(
                        "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    if stream.write_all(response.as_bytes()).is_ok() {
                        response_count += 1;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("transient server accept failed: {error}"),
            }
        }
        response_count
    });
    (base_url, TransientServer { stop, thread })
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

/// The query deadline the probe gives a provider. It is long enough for a
/// local request to be answered under any realistic scheduling, so the probe
/// does not race the first response.
const PROBE_QUERY_DEADLINE: Duration = Duration::from_millis(1_000);
/// Retries a probed provider is allowed. Their 50 ms-per-attempt backoff adds
/// up to minutes, so only the query deadline can end the call in time.
const PROBE_MAX_RETRIES: usize = 100;
/// Transport timeout of a probed provider, far beyond the query deadline, so
/// a timeout can only come from the deadline.
const PROBE_TRANSPORT_TIMEOUT: Duration = Duration::from_secs(60);
/// Hang guard for the provider call: well above the deadline and far below the
/// uncapped retry schedule.
const PROBE_HANG_GUARD: Duration = Duration::from_secs(15);

fn deadline_limits() -> CassieRuntimeLimits {
    CassieRuntimeLimits {
        query_timeout_ms: u64::try_from(PROBE_QUERY_DEADLINE.as_millis()).expect("deadline ms"),
        ..CassieRuntimeLimits::default()
    }
}

/// Runs `provider` against a server that always answers 429 and returns the
/// error and how long the call took.
fn run_deadline_probe(provider: Box<dyn EmbeddingProvider>) -> (EmbeddingError, Duration) {
    let limits = deadline_limits();
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let query_worker = std::thread::spawn(move || {
        let started = Instant::now();
        let controls =
            QueryExecutionControls::with_cancellation(&limits, started, query_cancellation);
        let result = provider.embed_documents_with_interruption(
            &["bounded input".to_string()],
            controls.interruption(),
        );
        let _ = result_tx.send((result, started.elapsed()));
    });
    let outcome = match result_rx.recv_timeout(PROBE_HANG_GUARD) {
        Ok((result, elapsed)) => (
            result.expect_err("deadline should interrupt provider retry"),
            elapsed,
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            cancellation.cancel();
            let cleanup = result_rx.recv_timeout(PROBE_HANG_GUARD);
            drop(query_worker);
            assert!(
                cleanup.is_ok(),
                "provider worker did not stop within the bounded cancellation cleanup"
            );
            panic!("provider call exceeded its hang guard");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            drop(query_worker);
            panic!("provider worker exited without reporting its result");
        }
    };
    drop(query_worker);
    outcome
}

/// The provider retried transient 429s and the query deadline, not the retry
/// budget or the transport timeout, ended the call.
fn assert_deadline_interrupted_retries(
    error: &EmbeddingError,
    elapsed: Duration,
    responses: usize,
) {
    assert!(matches!(error, EmbeddingError::Timeout { .. }));
    assert!(
        elapsed >= PROBE_QUERY_DEADLINE,
        "the call ended before its query deadline: {elapsed:?}"
    );
    assert!(
        elapsed < PROBE_HANG_GUARD,
        "provider retry outlived the query deadline: {elapsed:?}"
    );
    assert!(
        responses >= 1,
        "the provider must have received at least one transient 429 before the deadline"
    );
}

#[test]
fn should_clamp_openai_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, server) = transient_server();
    let provider = OpenAiProvider::with_config(OpenAiProviderConfig {
        api_key: "test-key".to_string(),
        model: "text-embedding-3-small".to_string(),
        timeout: PROBE_TRANSPORT_TIMEOUT,
        max_batch_size: 8,
        max_retries: PROBE_MAX_RETRIES,
        base_url,
    })
    .expect("configure OpenAI provider");

    // Act
    let (error, elapsed) = run_deadline_probe(Box::new(provider));
    let responses = server.stop();

    // Assert
    assert_deadline_interrupted_retries(&error, elapsed, responses);
}

#[test]
fn should_clamp_openai_compatible_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, server) = transient_server();
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        base_url,
        api_key: Some("test-key".to_string()),
        model: "compatible-test".to_string(),
        dimensions: 3,
        timeout: PROBE_TRANSPORT_TIMEOUT,
        max_batch_size: 8,
        max_retries: PROBE_MAX_RETRIES,
    })
    .expect("configure compatible provider");

    // Act
    let (error, elapsed) = run_deadline_probe(Box::new(provider));
    let responses = server.stop();

    // Assert
    assert_deadline_interrupted_retries(&error, elapsed, responses);
}

#[test]
fn should_clamp_tei_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, server) = transient_server();
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url,
        model: "tei-test".to_string(),
        dimensions: 3,
        timeout: PROBE_TRANSPORT_TIMEOUT,
        max_batch_size: 8,
        max_retries: PROBE_MAX_RETRIES,
    })
    .expect("configure TEI provider");

    // Act
    let (error, elapsed) = run_deadline_probe(Box::new(provider));
    let responses = server.stop();

    // Assert
    assert_deadline_interrupted_retries(&error, elapsed, responses);
}

#[test]
fn should_clamp_ollama_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, server) = transient_server();
    let provider = OllamaProvider::with_config(OllamaProviderConfig {
        base_url,
        model: "ollama-test".to_string(),
        dimensions: 3,
        timeout: PROBE_TRANSPORT_TIMEOUT,
        max_batch_size: 8,
        max_retries: PROBE_MAX_RETRIES,
    })
    .expect("configure Ollama provider");

    // Act
    let (error, elapsed) = run_deadline_probe(Box::new(provider));
    let responses = server.stop();

    // Assert
    assert_deadline_interrupted_retries(&error, elapsed, responses);
}

#[test]
fn should_clamp_voyage_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, server) = transient_server();
    let provider = VoyageProvider::with_config(VoyageProviderConfig {
        api_key: "test-key".to_string(),
        model: "voyage-test".to_string(),
        dimensions: 3,
        timeout: PROBE_TRANSPORT_TIMEOUT,
        max_batch_size: 8,
        max_retries: PROBE_MAX_RETRIES,
        base_url,
    })
    .expect("configure Voyage provider");

    // Act
    let (error, elapsed) = run_deadline_probe(Box::new(provider));
    let responses = server.stop();

    // Assert
    assert_deadline_interrupted_retries(&error, elapsed, responses);
}

#[test]
fn should_clamp_cohere_retry_backoff_to_query_deadline() {
    // Arrange
    let _guard = controlled_request_guard();
    let (base_url, server) = transient_server();
    let provider = CohereProvider::with_config(CohereProviderConfig {
        api_key: "test-key".to_string(),
        model: "cohere-test".to_string(),
        dimensions: 3,
        timeout: PROBE_TRANSPORT_TIMEOUT,
        max_batch_size: 8,
        max_retries: PROBE_MAX_RETRIES,
        base_url,
    })
    .expect("configure Cohere provider");

    // Act
    let (error, elapsed) = run_deadline_probe(Box::new(provider));
    let responses = server.stop();

    // Assert
    assert_deadline_interrupted_retries(&error, elapsed, responses);
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
        provider.embed_documents_with_interruption(
            &["bounded input".to_string()],
            controls.interruption(),
        )
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
