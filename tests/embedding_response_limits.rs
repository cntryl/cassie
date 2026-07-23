use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, CohereRuntimeConfig, EmbeddingsRuntimeConfig,
    OpenAiCompatibleRuntimeConfig, OpenAiRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
    VoyageRuntimeConfig,
};
use cassie::embeddings::{EmbeddingError, OpenAiConfig};

#[derive(Clone, Copy)]
enum Framing {
    Declared,
    Chunked,
}

fn spawn_server(
    response_status: u16,
    framing: Framing,
    request_count: usize,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock listener");
    let address = listener.local_addr().expect("mock address");
    let handle = thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().expect("mock accept");
            read_request(&mut stream);
            write_response(&mut stream, response_status, framing);
        }
    });
    (format!("http://{address}"), handle)
}

fn read_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut buffer).expect("request read");
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
        if expected.is_none() {
            if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let header_bytes = &request[..header_end + 4];
                let headers = String::from_utf8_lossy(header_bytes);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                expected = Some(header_end + 4 + content_length);
            }
        }
        if expected.is_some_and(|expected| request.len() >= expected) {
            return;
        }
    }
}

fn write_response(stream: &mut TcpStream, status: u16, framing: Framing) {
    let body = vec![b'x'; 64];
    let reason = if status == 200 { "OK" } else { "Server Error" };
    match framing {
        Framing::Declared => {
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("response headers");
            stream.write_all(&body).expect("response body");
        }
        Framing::Chunked => {
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
                body.len()
            )
            .expect("chunked response headers");
            stream.write_all(&body).expect("chunked response body");
            stream.write_all(b"\r\n0\r\n\r\n").expect("chunk ending");
        }
    }
    stream.flush().expect("response flush");
}

fn provider_configs(base_url: &str) -> Vec<EmbeddingsRuntimeConfig> {
    vec![
        EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
            config: OpenAiConfig {
                api_key: "test-key".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
            timeout_seconds: 2,
            max_batch_size: 1,
            max_retries: 1,
            base_url: Some(base_url.to_string()),
        }),
        EmbeddingsRuntimeConfig::OpenAiCompatible(OpenAiCompatibleRuntimeConfig {
            base_url: base_url.to_string(),
            api_key: None,
            model: "test-model".to_string(),
            dimensions: 2,
            timeout_seconds: 2,
            max_batch_size: 1,
            max_retries: 1,
        }),
        EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
            base_url: base_url.to_string(),
            model: "test-model".to_string(),
            dimensions: 2,
            timeout_seconds: 2,
            max_batch_size: 1,
            max_retries: 1,
        }),
        EmbeddingsRuntimeConfig::Ollama(SelfHostedEmbeddingRuntimeConfig {
            base_url: base_url.to_string(),
            model: "test-model".to_string(),
            dimensions: 2,
            timeout_seconds: 2,
            max_batch_size: 1,
            max_retries: 1,
        }),
        EmbeddingsRuntimeConfig::Voyage(VoyageRuntimeConfig {
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            dimensions: 2,
            timeout_seconds: 2,
            max_batch_size: 1,
            max_retries: 1,
            base_url: base_url.to_string(),
        }),
        EmbeddingsRuntimeConfig::Cohere(CohereRuntimeConfig {
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            dimensions: 2,
            timeout_seconds: 2,
            max_batch_size: 1,
            max_retries: 1,
            base_url: base_url.to_string(),
        }),
    ]
}

fn assert_remote_providers_reject_oversized_response(status: u16, framing: Framing) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let (base_url, server) = spawn_server(status, framing, 6);
    for (index, embeddings) in provider_configs(&base_url).into_iter().enumerate() {
        let data_dir = std::env::temp_dir().join(format!(
            "cassie-embedding-limit-{index}-{}",
            uuid::Uuid::new_v4()
        ));
        let config = CassieRuntimeConfig {
            embeddings,
            embeddings_max_response_bytes: 32,
            ..CassieRuntimeConfig::default()
        };
        let cassie =
            Cassie::new_with_data_dir_and_config(&data_dir, config).expect("provider cassie");

        let error = cassie
            .embedding_provider
            .embed_query("bounded response")
            .expect_err("oversized provider response");

        assert!(matches!(
            error,
            EmbeddingError::ResponseTooLarge {
                limit_bytes: 32,
                ..
            }
        ));
        drop(cassie);
        let _ = std::fs::remove_dir_all(data_dir);
    }
    server.join().expect("mock server");
}

#[test]
fn should_reject_declared_oversized_success_responses_for_every_remote_provider() {
    // Arrange / Act / Assert
    assert_remote_providers_reject_oversized_response(200, Framing::Declared);
}

#[test]
fn should_reject_chunked_oversized_success_responses_for_every_remote_provider() {
    // Arrange / Act / Assert
    assert_remote_providers_reject_oversized_response(200, Framing::Chunked);
}

#[test]
fn should_reject_declared_oversized_error_responses_for_every_remote_provider() {
    // Arrange / Act / Assert
    assert_remote_providers_reject_oversized_response(500, Framing::Declared);
}

#[test]
fn should_reject_chunked_oversized_error_responses_for_every_remote_provider() {
    // Arrange / Act / Assert
    assert_remote_providers_reject_oversized_response(500, Framing::Chunked);
}
