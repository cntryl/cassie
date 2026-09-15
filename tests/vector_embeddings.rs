// Consolidated integration suite: vector_embeddings.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/embedding_local.rs.
mod embedding_local {
    use cassie::embeddings::local::{LocalProvider, LocalProviderConfig};
    use cassie::embeddings::EmbeddingProvider;

    fn magnitude(values: &[f32]) -> f32 {
        values.iter().map(|value| value * value).sum::<f32>().sqrt()
    }

    #[test]
    fn should_generate_identical_local_embeddings_given_identical_input_and_configuration() {
        // Arrange
        let provider = LocalProvider::with_config(LocalProviderConfig {
            model: "cassie-local-hash-v1".to_string(),
            dimensions: 8,
        })
        .expect("provider should configure");
        let inputs = vec!["alpha".to_string(), "beta".to_string()];

        // Act
        let first = provider
            .embed_documents(&inputs)
            .expect("first embedding pass");
        let second = provider
            .embed_documents(&inputs)
            .expect("second embedding pass");

        // Assert
        assert_eq!(provider.provider_name(), "local");
        assert_eq!(provider.model_name(), "cassie-local-hash-v1");
        assert_eq!(provider.dimensions(), 8);
        assert_eq!(first.len(), second.len());
        for (lhs, rhs) in first.iter().zip(&second) {
            assert_eq!(lhs.values, rhs.values);
        }
        assert_eq!(first[0].values.len(), 8);
    }

    #[test]
    fn should_normalize_non_zero_local_embeddings_to_unit_length() {
        // Arrange
        let provider = LocalProvider::with_config(LocalProviderConfig {
            model: "cassie-local-hash-v1".to_string(),
            dimensions: 16,
        })
        .expect("provider should configure");

        // Act
        let document = provider
            .embed_documents(&["alpha".to_string()])
            .expect("document embedding")
            .remove(0);
        let query = provider.embed_query("alpha").expect("query embedding");

        // Assert
        assert!((magnitude(&document.values) - 1.0).abs() < 1e-5);
        assert!((magnitude(&query.values) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn should_distinguish_local_query_embeddings_from_document_embeddings() {
        // Arrange
        let provider = LocalProvider::with_config(LocalProviderConfig {
            model: "cassie-local-hash-v1".to_string(),
            dimensions: 12,
        })
        .expect("provider should configure");

        // Act
        let document = provider
            .embed_documents(&["alpha".to_string()])
            .expect("document embedding")
            .remove(0);
        let query = provider.embed_query("alpha").expect("query embedding");

        // Assert
        assert_ne!(document.values, query.values);
        assert_eq!(document.values.len(), 12);
        assert_eq!(query.values.len(), 12);
    }

    #[test]
    fn should_handle_empty_local_embedding_input() {
        // Arrange
        let provider = LocalProvider::with_config(LocalProviderConfig {
            model: "cassie-local-hash-v1".to_string(),
            dimensions: 8,
        })
        .expect("provider should configure");

        // Act
        let embeddings = provider
            .embed_documents(&[])
            .expect("empty embedding batch should succeed");

        // Assert
        assert!(embeddings.is_empty());
    }

    #[test]
    fn should_reject_invalid_local_embedding_dimensions() {
        // Arrange
        let invalid_dimensions = 0;

        // Act
        let result = LocalProvider::with_config(LocalProviderConfig {
            model: "cassie-local-hash-v1".to_string(),
            dimensions: invalid_dimensions,
        });

        // Assert
        assert!(result.is_err());
    }
}

// Formerly tests/embedding_response_limits.rs.
mod embedding_response_limits {
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
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
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
}

// Formerly tests/embedding_self_hosted.rs.
mod embedding_self_hosted {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use cassie::embeddings::cohere::{CohereProvider, CohereProviderConfig};
    use cassie::embeddings::compatible::{
        OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
    };
    use cassie::embeddings::ollama::{OllamaProvider, OllamaProviderConfig};
    use cassie::embeddings::tei::{TeiProvider, TeiProviderConfig};
    use cassie::embeddings::voyage::{VoyageProvider, VoyageProviderConfig};
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
        requests: Arc<Mutex<Vec<HttpRequest>>>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl MockEmbeddingServer {
        fn spawn(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock embedding server");
            let base_url = format!(
                "http://{}",
                listener.local_addr().expect("mock server address")
            );
            let requests = Arc::new(Mutex::new(Vec::new()));
            let request_sink = Arc::clone(&requests);
            let thread = thread::spawn(move || {
                for response in responses {
                    let (mut stream, _) = listener.accept().expect("mock accept");
                    let request = read_http_request(&mut stream);
                    if let Some(expected) = &response.expected_authorization {
                        assert!(
                            request.headers.to_ascii_lowercase().contains(&format!(
                                "authorization: {}",
                                expected.to_ascii_lowercase()
                            )),
                            "expected authorization header"
                        );
                    }
                    request_sink
                        .lock()
                        .expect("recorded requests lock")
                        .push(request.clone());
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
                requests,
                thread: Some(thread),
            }
        }

        fn base_url(&self) -> String {
            self.base_url.clone()
        }

        fn recorded_requests(&self) -> Vec<HttpRequest> {
            self.requests
                .lock()
                .expect("recorded requests lock")
                .clone()
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
    fn should_embed_documents_with_voyage_provider() {
        // Arrange
        let server = MockEmbeddingServer::spawn(vec![MockResponse::requiring_authorization(
            &serde_json::json!({
                "data": [
                    {"index": 1, "embedding": [0.4, 0.5, 0.6]},
                    {"index": 0, "embedding": [0.1, 0.2, 0.3]}
                ]
            }),
            "Bearer voyage-secret",
        )]);
        let provider = VoyageProvider::with_config(VoyageProviderConfig {
            api_key: "voyage-secret".to_string(),
            model: "voyage-3.5-lite".to_string(),
            dimensions: 3,
            timeout: std::time::Duration::from_secs(1),
            max_batch_size: 8,
            max_retries: 1,
            base_url: server.base_url(),
        })
        .expect("provider should configure");
        let inputs = vec!["alpha".to_string(), "beta".to_string()];

        // Act
        let embeddings = provider
            .embed_documents(&inputs)
            .expect("embeddings should return");
        let requests = server.recorded_requests();
        let request_body = request_json(&requests[0]);

        // Assert
        assert_eq!(provider.provider_name(), "voyage");
        assert_eq!(embeddings[0].values, vec![0.1, 0.2, 0.3]);
        assert_eq!(embeddings[1].values, vec![0.4, 0.5, 0.6]);
        assert_eq!(request_body["input_type"], "document");
        assert_eq!(request_body["output_dimension"], 3);
        assert_eq!(request_body["model"], "voyage-3.5-lite");
    }

    #[test]
    fn should_embed_query_with_voyage_provider_query_input_type() {
        // Arrange
        let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!({
            "data": [
                {"index": 0, "embedding": [0.1, 0.2, 0.3]}
            ]
        }))]);
        let provider = VoyageProvider::with_config(VoyageProviderConfig {
            api_key: "voyage-secret".to_string(),
            model: "voyage-3.5-lite".to_string(),
            dimensions: 3,
            timeout: std::time::Duration::from_secs(1),
            max_batch_size: 8,
            max_retries: 1,
            base_url: server.base_url(),
        })
        .expect("provider should configure");

        // Act
        let embedding = provider.embed_query("alpha").expect("query embedding");
        let requests = server.recorded_requests();
        let request_body = request_json(&requests[0]);

        // Assert
        assert_eq!(embedding.values, vec![0.1, 0.2, 0.3]);
        assert_eq!(request_body["input_type"], "query");
        assert_eq!(request_body["input"][0], "alpha");
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
    fn should_embed_documents_with_cohere_provider() {
        // Arrange
        let server = MockEmbeddingServer::spawn(vec![MockResponse::requiring_authorization(
            &serde_json::json!({
                "embeddings": {
                    "float": [
                        [0.1, 0.2, 0.3],
                        [0.4, 0.5, 0.6]
                    ]
                }
            }),
            "Bearer cohere-secret",
        )]);
        let provider = CohereProvider::with_config(CohereProviderConfig {
            api_key: "cohere-secret".to_string(),
            model: "embed-v4.0".to_string(),
            dimensions: 3,
            timeout: std::time::Duration::from_secs(1),
            max_batch_size: 8,
            max_retries: 1,
            base_url: server.base_url(),
        })
        .expect("provider should configure");
        let inputs = vec!["alpha".to_string(), "beta".to_string()];

        // Act
        let embeddings = provider
            .embed_documents(&inputs)
            .expect("embeddings should return");
        let requests = server.recorded_requests();
        let request_body = request_json(&requests[0]);

        // Assert
        assert_eq!(provider.provider_name(), "cohere");
        assert_eq!(embeddings[0].values, vec![0.1, 0.2, 0.3]);
        assert_eq!(embeddings[1].values, vec![0.4, 0.5, 0.6]);
        assert_eq!(request_body["input_type"], "search_document");
        assert_eq!(request_body["output_dimension"], 3);
        assert_eq!(request_body["embedding_types"][0], "float");
    }

    #[test]
    fn should_embed_query_with_cohere_provider_query_input_type() {
        // Arrange
        let server = MockEmbeddingServer::spawn(vec![MockResponse::ok(&serde_json::json!({
            "embeddings": {
                "float": [
                    [0.1, 0.2, 0.3]
                ]
            }
        }))]);
        let provider = CohereProvider::with_config(CohereProviderConfig {
            api_key: "cohere-secret".to_string(),
            model: "embed-v4.0".to_string(),
            dimensions: 3,
            timeout: std::time::Duration::from_secs(1),
            max_batch_size: 8,
            max_retries: 1,
            base_url: server.base_url(),
        })
        .expect("provider should configure");

        // Act
        let embedding = provider.embed_query("alpha").expect("query embedding");
        let requests = server.recorded_requests();
        let request_body = request_json(&requests[0]);

        // Assert
        assert_eq!(embedding.values, vec![0.1, 0.2, 0.3]);
        assert_eq!(request_body["input_type"], "search_query");
        assert_eq!(request_body["texts"][0], "alpha");
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

    #[derive(Clone)]
    struct HttpRequest {
        headers: String,
        body: Vec<u8>,
    }

    fn request_json(request: &HttpRequest) -> serde_json::Value {
        serde_json::from_slice(&request.body).expect("request body json")
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
}

// Formerly tests/embedding_validation.rs.
mod embedding_validation {
    use super::support_sql as support;
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig, VoyageRuntimeConfig,
    };
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::types::{DataType, FieldSchema, Schema};
    use support::*;

    fn openai_runtime(base_url: String) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
            config: OpenAiConfig {
                api_key: "test-key".to_string(),
                model: DEFAULT_EMBEDDING_MODEL.to_string(),
            },
            timeout_seconds: 1,
            max_batch_size: 1,
            max_retries: 1,
            base_url: Some(base_url),
        });
        config
    }

    fn voyage_runtime() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::Voyage(VoyageRuntimeConfig {
            api_key: "voyage-test-key".to_string(),
            model: "voyage-3.5-lite".to_string(),
            dimensions: 1024,
            timeout_seconds: 1,
            max_batch_size: 1,
            max_retries: 1,
            base_url: "http://127.0.0.1:1".to_string(),
        });
        config
    }

    fn ensure_collection(cassie: &Cassie, collection: &str, schema: &Schema) {
        // Arrange.
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
    }

    fn vector_index_record(
        collection: &str,
        provider: &str,
        dimensions: usize,
        metric: DistanceMetric,
    ) -> VectorIndexRecord {
        VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "content".to_string(),
            metadata: VectorIndexMetadata {
                provider: provider.to_string(),
                model: DEFAULT_EMBEDDING_MODEL.to_string(),
                dimensions,
                metric,
                index_type: VectorIndexType::BruteForce,
                hnsw: None,
                hnsw_graph: None,
                ivfflat: None,
                ivfflat_training: None,
            },
        }
    }

    #[test]
    fn should_reject_ingest_when_query_provider_model_mismatch() {
        // Arrange
        use_local_storage();
        let path = data_dir("provider_mismatch");
        let path_for_cleanup = path.clone();
        let openai = Cassie::new_with_data_dir_and_config(
            &path,
            openai_runtime("http://127.0.0.1:1".to_string()),
        )
        .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            // Arrange (seed existing index metadata)
            let collection = "provider_mismatch_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "content".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(1536),
                        nullable: true,
                    },
                ],
            };

            openai.startup().unwrap();
            ensure_collection(&openai, collection, &schema);

            openai
                .midge
                .put_vector_index(vector_index_record(
                    collection,
                    "openai",
                    1536,
                    DistanceMetric::Cosine,
                ))
                .unwrap();
            openai.register_vector_index(vector_index_record(
                collection,
                "openai",
                1536,
                DistanceMetric::Cosine,
            ));
        });

        drop(openai);

        let voyage = Cassie::new_with_data_dir_and_config(&path, voyage_runtime()).unwrap();
        runtime.block_on(async {
            // Act
            voyage.startup().unwrap();
            let result = voyage.ingest_document(
                "provider_mismatch_docs",
                serde_json::json!({"content": "sample text"}),
            );

            // Assert
            assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_reject_ingest_when_dimensions_change() {
        // Arrange
        use_local_storage();
        let path = data_dir("dimension_mismatch");
        let path_for_cleanup = path.clone();
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            openai_runtime("http://127.0.0.1:1".to_string()),
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            // Act
            let collection = "dimension_mismatch_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "content".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(1536),
                        nullable: true,
                    },
                ],
            };

            cassie.startup().unwrap();
            ensure_collection(&cassie, collection, &schema);
            let index_record = vector_index_record(collection, "openai", 2, DistanceMetric::Cosine);
            cassie.midge.put_vector_index(index_record.clone()).unwrap();
            cassie.register_vector_index(index_record);

            // Assert
            let result =
                cassie.ingest_document(collection, serde_json::json!({"content": "sample text"}));
            assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_reject_query_when_metric_different() {
        // Arrange
        use_local_storage();
        let path = data_dir("metric_mismatch");
        let path_for_cleanup = path.clone();
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            openai_runtime("http://127.0.0.1:1".to_string()),
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            // Act
            let collection = "metric_mismatch_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "content".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(1536),
                        nullable: true,
                    },
                ],
            };

            cassie.startup().unwrap();
            ensure_collection(&cassie, collection, &schema);
            let index_record =
                vector_index_record(collection, "openai", 1536, DistanceMetric::Cosine);
            cassie.midge.put_vector_index(index_record.clone()).unwrap();
            cassie.register_vector_index(index_record);

            // Assert
            let result = cassie.execute_vector_search(
                collection,
                "embedding",
                "query",
                Some(DistanceMetric::Dot),
                10,
                0,
            );
            assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }
}

// Formerly tests/hnsw_indexes.rs.
mod hnsw_indexes {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::embeddings::{
        DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    };
    use cassie::midge::adapter::{
        document_write_failure_point_test_guard, set_document_write_failure_point,
        DocumentWriteFailurePoint, StorageFamily,
    };
    use cassie::types::{DataType, FieldSchema, Schema, Value};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_clean_batched_hnsw_sidecars_after_failed_publication_retry() {
        // Arrange
        let _failpoint_guard = document_write_failure_point_test_guard();
        use_local_storage();
        let path = data_dir("hnsw_batched_build_retry");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let collection = "hnsw_batched_build_retry";
        register_hnsw_collection(&cassie, collection);
        let canonical_collection = canonical_hnsw_collection(collection);
        let documents = (0..5_001)
            .map(|index| {
                let ordinate = f64::from(u32::try_from(index % 97).expect("small ordinate")) + 1.0;
                (
                    Some(format!("document-{index:05}")),
                    serde_json::json!({
                        "content": format!("document-{index:05}"),
                        "embedding": [1.0, ordinate, 0.5]
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_documents(&canonical_collection, documents)
            .expect("seed documents");
        let record = hnsw_index_record(collection, 2);
        let normalized_prefix = cassie
            .midge
            .normalized_vector_prefix_for_diagnostics(&canonical_collection, "embedding")
            .expect("normalized-vector prefix");
        let node_prefix = cassie
            .midge
            .hnsw_node_prefix_for_diagnostics(&canonical_collection, "embedding")
            .expect("node prefix");

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorState));
        let failed = cassie.midge.put_vector_index(record.clone());
        let unpublished = cassie
            .midge
            .get_vector_index(&canonical_collection, "embedding")
            .expect("read unpublished index");
        let staged_nodes = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &node_prefix)
            .expect("scan staged nodes");
        set_document_write_failure_point(None);
        cassie
            .midge
            .put_vector_index(record)
            .expect("retry index build");
        let published = stored_hnsw_index(&cassie, collection);
        cassie
            .midge
            .delete_vector_index(&canonical_collection, "embedding")
            .expect("delete batched vector sidecars");
        let remaining_normalized = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &normalized_prefix)
            .expect("scan remaining normalized vectors");
        let remaining_nodes = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &node_prefix)
            .expect("scan remaining HNSW nodes");

        // Assert
        let failed = failed.expect_err("manifest publication should fail");
        assert!(failed.to_string().contains("injected test failure"));
        assert!(unpublished.is_none());
        assert_eq!(staged_nodes.len(), 5_001);
        assert_eq!(
            published
                .metadata
                .hnsw_graph
                .expect("published graph")
                .row_count,
            5_001
        );
        assert!(remaining_normalized.is_empty());
        assert!(remaining_nodes.is_empty());

        let _ = std::fs::remove_dir_all(path);
    }

    fn canonical_hnsw_collection(collection: &str) -> String {
        canonical_relation_name("postgres", "public", collection)
    }

    fn register_hnsw_collection(cassie: &Cassie, collection: &str) {
        let collection = canonical_hnsw_collection(collection);
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(3),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            &collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
    }

    fn put_hnsw_document(cassie: &Cassie, collection: &str, id: &str, embedding: [f64; 3]) {
        let collection = canonical_hnsw_collection(collection);
        cassie
            .midge
            .put_document(
                &collection,
                Some(id.to_string()),
                serde_json::json!({"content": id, "embedding": embedding}),
            )
            .unwrap();
    }

    fn hnsw_index_record(collection: &str, ef_search: usize) -> VectorIndexRecord {
        VectorIndexRecord {
            collection: canonical_hnsw_collection(collection),
            field: "embedding".to_string(),
            source_field: "content".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::L2,
                index_type: VectorIndexType::Hnsw,
                hnsw: Some(HnswIndexOptions {
                    version: 1,
                    m: 2,
                    ef_construction: 4,
                    ef_search,
                }),
                hnsw_graph: None,
                ivfflat: None,
                ivfflat_training: None,
            },
        }
    }

    fn put_hnsw_index(cassie: &Cassie, collection: &str, ef_search: usize) -> VectorIndexRecord {
        let record = hnsw_index_record(collection, ef_search);
        cassie.midge.put_vector_index(record.clone()).unwrap();
        cassie.register_vector_index(record.clone());
        record
    }

    fn stored_hnsw_index(cassie: &Cassie, collection: &str) -> VectorIndexRecord {
        let collection = canonical_hnsw_collection(collection);
        cassie
            .midge
            .get_vector_index(&collection, "embedding")
            .unwrap()
            .expect("hnsw vector index should persist")
    }

    fn clear_stored_hnsw_graph(cassie: &Cassie, collection: &str) {
        mutate_stored_hnsw_index(cassie, collection, |record| {
            record.metadata.hnsw_graph = None;
        });
    }

    fn mutate_stored_hnsw_index(
        cassie: &Cassie,
        collection: &str,
        mut mutate: impl FnMut(&mut VectorIndexRecord),
    ) {
        let collection = canonical_hnsw_collection(collection);
        let mut record = cassie
            .midge
            .get_vector_index(&collection, "embedding")
            .unwrap()
            .expect("stored vector index metadata should exist");
        mutate(&mut record);
        cassie
            .midge
            .put_vector_index_state(
                &collection,
                "embedding",
                cassie::embeddings::VectorIndexState {
                    built_generation: 0,
                    hnsw_graph: record.metadata.hnsw_graph,
                    ivfflat_training: record.metadata.ivfflat_training,
                },
            )
            .unwrap();
    }

    fn assert_hnsw_fallback_query(
        cassie: &Cassie,
        collection: &str,
        expected_reason: &str,
        before: &serde_json::Value,
    ) {
        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            &format!(
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"
            ),
            vec![],
        )
        .unwrap();
        let after = cassie.metrics();

        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_eq!(
            after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
                - before["vector"]["hnsw_fallbacks"].as_u64().unwrap(),
            1
        );
        assert_eq!(
            after["vector"]["last_fallback_reason"].as_str(),
            Some(expected_reason)
        );
    }

    #[test]
    fn should_hydrate_persisted_hnsw_graph_state_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_graph_restart");
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "hnsw_graph_restart";
            register_hnsw_collection(&cassie, collection);
            put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
            put_hnsw_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
            put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
            put_hnsw_index(&cassie, collection, 2);

            let stored = stored_hnsw_index(&cassie, collection);
            let graph = stored.metadata.hnsw_graph.expect("hnsw graph state");
            assert_eq!(graph.row_count, 3);
            assert_eq!(graph.dimensions, 3);
            assert_eq!(graph.metric, DistanceMetric::L2);
            assert!(graph.entry_point.is_some());
            assert_eq!(graph.nodes.len(), 3);
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let stored = stored_hnsw_index(&restarted, "hnsw_graph_restart");

        // Assert
        let graph = stored
            .metadata
            .hnsw_graph
            .expect("hydrated hnsw graph state");
        assert_eq!(graph.row_count, 3);
        assert_eq!(graph.nodes.len(), 3);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_store_hnsw_graph_state_in_the_data_family() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_graph_data_family");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let collection = "hnsw_graph_data_family";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);

        // Act
        put_hnsw_index(&cassie, collection, 2);
        let raw_metadata = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .expect("schema scan")
            .into_iter()
            .find_map(|(_key, value)| serde_json::from_slice::<VectorIndexRecord>(&value).ok())
            .expect("vector index metadata");
        let state = cassie
            .midge
            .get_vector_index_state(&canonical_hnsw_collection(collection), "embedding")
            .expect("read state")
            .expect("persisted state");

        // Assert
        assert!(raw_metadata.metadata.hnsw_graph.is_none());
        assert!(state.hnsw_graph.is_some());
        assert!(state.ivfflat_training.is_none());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_refresh_hnsw_graph_after_document_mutations() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_graph_refresh");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_graph_refresh";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        assert_eq!(
            stored_hnsw_index(&cassie, collection)
                .metadata
                .hnsw_graph
                .unwrap()
                .row_count,
            2
        );

        // Act
        put_hnsw_document(&cassie, collection, "new-nearest", [0.9, 0.0, 0.0]);
        let after_insert = stored_hnsw_index(&cassie, collection);
        cassie
            .midge
            .delete_document(&canonical_hnsw_collection(collection), "far")
            .expect("delete document");
        let after_delete = stored_hnsw_index(&cassie, collection);

        // Assert
        assert_eq!(after_insert.metadata.hnsw_graph.unwrap().row_count, 3);
        assert_eq!(after_delete.metadata.hnsw_graph.unwrap().row_count, 2);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_hnsw_reads_safe_during_concurrent_mutation() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_concurrent_mutation");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_concurrent_mutation";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        let cassie = std::sync::Arc::new(cassie);

        // Act
        let readers = (0..4)
        .map(|_| {
            let cassie = std::sync::Arc::clone(&cassie);
            std::thread::spawn(move || {
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_concurrent_mutation ORDER BY distance ASC LIMIT 1",
                        vec![],
                    )
                    .unwrap()
                    .rows[0][0]
                    .clone()
            })
        })
        .collect::<Vec<_>>();
        let writer = {
            let cassie = std::sync::Arc::clone(&cassie);
            std::thread::spawn(move || {
                put_hnsw_document(
                    cassie.as_ref(),
                    "hnsw_concurrent_mutation",
                    "new-nearest",
                    [0.99, 0.0, 0.0],
                );
            })
        };
        writer.join().unwrap();
        let results = readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(results.len(), 4);
        assert!(results.into_iter().all(|value| {
            value == Value::String("near".to_string())
                || value == Value::String("new-nearest".to_string())
        }));
        assert_eq!(
            stored_hnsw_index(cassie.as_ref(), collection)
                .metadata
                .hnsw_graph
                .unwrap()
                .row_count,
            4
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_use_hnsw_graph_for_sql_vector_top_k_with_bounded_candidates() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_sql_topk");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_sql_topk";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "middle", [0.6, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_sql_topk ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_eq!(
            after["vector"]["hnsw_executions"].as_u64().unwrap()
                - before["vector"]["hnsw_executions"].as_u64().unwrap(),
            1
        );
        assert!(
            after["vector"]["candidate_count_total"].as_u64().unwrap()
                - before["vector"]["candidate_count_total"].as_u64().unwrap()
                < 4
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_hnsw_sql_top_k_query_dimension_mismatch() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_sql_topk_dimension_mismatch");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_sql_topk_dimension_mismatch";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        let session = cassie.create_session("tester", None);

        // Act
        let error = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM hnsw_sql_topk_dimension_mismatch ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .expect_err("dimension mismatch should fail");

        // Assert
        let error = error.to_string();
        assert!(error.contains("vector_distance query for field 'embedding' on collection"));
        assert!(error.contains("expects 3 dimensions but received 2"));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_deterministically_when_hnsw_graph_is_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_missing_graph_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_missing_graph_fallback";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        clear_stored_hnsw_graph(&cassie, collection);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_missing_graph_fallback ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_eq!(
            after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
                - before["vector"]["hnsw_fallbacks"].as_u64().unwrap(),
            1
        );
        assert_eq!(
            after["vector"]["last_fallback_reason"].as_str(),
            Some("missing-graph")
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_same_row_count_hnsw_graph_fingerprint_is_stale() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_stale_fingerprint_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_stale_fingerprint_fallback";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        mutate_stored_hnsw_index(&cassie, collection, |record| {
            let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
            graph.source_fingerprint ^= 1;
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "stale-source-fingerprint";

        // Assert
        assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_hnsw_graph_neighbor_reference_is_corrupt() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_corrupt_neighbor_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_corrupt_neighbor_fallback";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        mutate_stored_hnsw_index(&cassie, collection, |record| {
            let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
            graph.nodes[0].layers[0].push("missing-neighbor".to_string());
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "unknown-neighbor-id";

        // Assert
        assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_hnsw_graph_entry_point_is_invalid() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_invalid_entry_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_invalid_entry_fallback";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        mutate_stored_hnsw_index(&cassie, collection, |record| {
            let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
            graph.entry_point = Some("missing-entry".to_string());
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "missing-entry-point";

        // Assert
        assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_hnsw_graph_max_layer_is_invalid() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_invalid_max_layer_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_invalid_max_layer_fallback";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);
        mutate_stored_hnsw_index(&cassie, collection, |record| {
            let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
            graph.max_layer = graph.max_layer.saturating_add(1);
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "inconsistent-max-layer";

        // Assert
        assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_store_hnsw_nodes_as_point_readable_binary_records() {
        // Arrange
        let (cassie, path, collection) = hnsw_layout_fixture("hnsw_point_nodes");

        // Act
        put_hnsw_index(&cassie, collection, 2);
        let (_, entries, _, node_count, monolithic_graph_count) =
            inspect_hnsw_layout(&cassie, collection);

        // Assert
        assert_eq!(node_count, 2);
        assert_eq!(monolithic_graph_count, 0);
        assert!(entries.iter().all(|(_, raw)| raw.first() != Some(&0x7b)));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_store_hnsw_manifest_as_binary_record() {
        // Arrange
        let (cassie, path, collection) = hnsw_layout_fixture("hnsw_binary_manifest");

        // Act
        put_hnsw_index(&cassie, collection, 2);
        let (_, _, state_value, _, _) = inspect_hnsw_layout(&cassie, collection);

        // Assert
        assert_eq!(state_value.first(), Some(&3));
        assert_ne!(state_value.first(), Some(&0x7b));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_omit_names_from_hnsw_hot_keys() {
        // Arrange
        let (cassie, path, collection) = hnsw_layout_fixture("hnsw_numeric_keys");

        // Act
        put_hnsw_index(&cassie, collection, 2);
        let (prefix, _, _, _, _) = inspect_hnsw_layout(&cassie, collection);

        // Assert
        assert!(!prefix
            .windows(collection.len())
            .any(|window| window == collection.as_bytes()));
        assert!(!prefix
            .windows("embedding".len())
            .any(|window| window == b"embedding"));

        let _ = std::fs::remove_dir_all(path);
    }

    fn hnsw_layout_fixture(label: &str) -> (Cassie, String, &'static str) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_layout_records";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        (cassie, path, collection)
    }

    type HnswLayoutInspection = (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>, Vec<u8>, usize, usize);

    fn inspect_hnsw_layout(cassie: &Cassie, collection: &str) -> HnswLayoutInspection {
        let prefix = cassie
            .midge
            .hnsw_node_prefix_for_diagnostics(collection, "embedding")
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .unwrap();
        let state_key = cassie
            .midge
            .vector_state_key_for_diagnostics(collection, "embedding")
            .unwrap();
        let state_value = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &state_key)
            .unwrap()
            .into_iter()
            .find_map(|(key, value)| (key == state_key).then_some(value))
            .expect("vector state manifest");
        let node_count = entries
            .iter()
            .filter(|(_, raw)| raw.first() == Some(&2))
            .count();
        let monolithic_graph_count = entries
            .iter()
            .filter(|(_, raw)| {
                serde_json::from_slice::<cassie::embeddings::HnswGraphState>(raw).is_ok()
            })
            .count();
        (
            prefix,
            entries,
            state_value,
            node_count,
            monolithic_graph_count,
        )
    }

    #[test]
    fn should_scale_hnsw_query_reads_with_reachable_nodes_not_corpus_size() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_point_read_scaling");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_point_read_scaling";
        register_hnsw_collection(&cassie, collection);
        for index in 0..32 {
            let value = if index == 0 { 1.0 } else { -1.0 };
            put_hnsw_document(
                &cassie,
                collection,
                &format!("doc-{index}"),
                [value, 0.0, 0.0],
            );
        }
        put_hnsw_index(&cassie, collection, 2);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_point_read_scaling ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(reads < 32, "expected bounded point reads, observed {reads}");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_remove_hnsw_point_nodes_when_index_is_dropped() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_point_nodes_drop_cleanup");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_point_nodes_drop_cleanup";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);

        // Act
        cassie
            .midge
            .delete_vector_index(&canonical_hnsw_collection(collection), "embedding")
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap();

        // Assert
        assert!(!entries.iter().any(|(_, raw)| {
            serde_json::from_slice::<cassie::embeddings::HnswGraphNode>(raw).is_ok()
        }));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_batch_vector_index_drop_sidecar_transactions() {
        // Arrange
        let metadata = include_str!("../src/midge/adapter/metadata.rs");
        let vector_indexes = include_str!("../src/midge/adapter/vector_indexes.rs");
        let drop_start = metadata
            .find("pub fn delete_vector_index")
            .expect("vector index delete implementation");
        let drop_end = metadata[drop_start..]
            .find("pub fn put_projection_comparison_report")
            .map(|offset| drop_start + offset)
            .expect("next metadata function");
        let drop_implementation = &metadata[drop_start..drop_end];

        // Act
        let delegates_sidecars = drop_implementation
            .contains("delete_vector_sidecars_in_batches(collection, &prefixes)");
        let batches_deletes =
            vector_indexes.contains("for keys in keys.chunks(VECTOR_INDEX_BUILD_WRITE_BATCH_SIZE)");
        let pages_sidecars = vector_indexes.contains("raw_scan_prefix_page_for_collection(");
        let manifest_is_deleted_after_sidecars = drop_implementation
            .find("delete_vector_sidecars_in_batches")
            .zip(drop_implementation.find("vector_index_state_key"))
            .is_some_and(|(sidecars, manifest)| sidecars < manifest);

        // Assert
        assert!(delegates_sidecars);
        assert!(batches_deletes);
        assert!(pages_sidecars);
        assert!(manifest_is_deleted_after_sidecars);
    }
}

// Formerly tests/integration_sql_hybrid_query.rs.
mod integration_sql_hybrid_query {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    fn hybrid_params(query: &str) -> Vec<Value> {
        vec![
            Value::String(query.to_string()),
            Value::Vector(Vector::new(vec![1.0, 0.0])),
        ]
    }

    #[test]
    fn should_order_hybrid_top_k_by_score_with_limit() {
        // Arrange
        use_local_storage();
        let path = data_dir("hybrid_top_k_limit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_limit";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_top_k_limit ORDER BY score DESC LIMIT 1",
                hybrid_params("red"),
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_generate_hybrid_candidates_from_text_matches() {
        // Arrange
        use_local_storage();
        let path = data_dir("hybrid_text_candidates");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_text_candidates";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("text_match".to_string()),
                serde_json::json!({"body": "red", "embedding": [100.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("vector_only".to_string()),
                serde_json::json!({"body": "blue", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        let before = cassie.metrics();
        let before_candidates = before["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_text_candidates ORDER BY score DESC LIMIT 1",
                hybrid_params("red"),
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("text_match".to_string()));
        assert_eq!(
            after["hybrid"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_mixed_text_vector_execution_stages() {
        // Arrange
        use_local_storage();
        let path = data_dir("hybrid_explain_mixed_stages");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_explain_mixed_stages";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };
        cassie.midge.create_collection(collection, schema.clone()).unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_explain_mixed_stages ORDER BY score DESC LIMIT 5",
                hybrid_params("red"),
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("mixed_execution=true"));
        assert!(plan.contains("mixed_stages=candidate_generation>exact_scoring>ordering"));
        assert!(plan.contains(">limit"));
        assert!(plan.contains("exact_baseline=source_row_exact_baseline"));
        assert!(plan.contains("projection_freshness=unavailable"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_hybrid_text_candidate_without_vector() {
        // Arrange
        use_local_storage();
        let path = data_dir("hybrid_missing_vector");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_missing_vector";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("text_without_vector".to_string()),
                serde_json::json!({"body": "red"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("ignored_non_match".to_string()),
                serde_json::json!({"body": "blue"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_missing_vector ORDER BY score DESC LIMIT 1",
                hybrid_params("red"),
            );

        // Assert
        let error = result.expect_err("text candidate should require a vector");
        assert!(error.to_string().contains("vector_score expects vector"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    fn bounded_hybrid_fixture() -> (Cassie, String, &'static str) {
        bounded_hybrid_fixture_with_limits(100_000, None)
    }

    fn bounded_hybrid_fixture_with_max(max_candidates: usize) -> (Cassie, String, &'static str) {
        bounded_hybrid_fixture_with_limits(max_candidates, None)
    }

    fn bounded_hybrid_fixture_with_limits(
        max_candidates: usize,
        query_memory_budget_bytes: Option<usize>,
    ) -> (Cassie, String, &'static str) {
        use_local_storage();
        let path = data_dir("hybrid_bounded_candidates");
        let mut config = CassieRuntimeConfig::from_env().unwrap();
        config.limits.adaptive_candidate_max = max_candidates;
        if let Some(query_memory_budget_bytes) = query_memory_budget_bytes {
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        }
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "hybrid_bounded_candidates";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        for index in 0..64 {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(format!("d{index}")),
                    serde_json::json!({
                        "body": if index < 2 { "alpha marker" } else { "unrelated" },
                        "embedding": [1.0, 0.0],
                        "status": if index == 0 { "approved" } else { "pending" }
                    }),
                )
                .unwrap();
        }
        let fulltext = IndexMeta {
            collection: collection.to_string(),
            name: "fulltext_body_idx".to_string(),
            field: "body".to_string(),
            fields: vec!["body".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::FullText,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        cassie.midge.put_index(&fulltext).unwrap();
        cassie.catalog.register_index(fulltext);
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "body".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 2,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::IvfFlat,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: Some(cassie::embeddings::IvfFlatIndexOptions {
                        version: 1,
                        lists: 2,
                        probes: 1,
                        training_sample_size: 64,
                        training_seed: 1,
                    }),
                    ivfflat_training: None,
                },
            })
            .unwrap();
        (cassie, path, collection)
    }

    fn corrupt_matching_data_value(
        cassie: &Cassie,
        predicate: impl Fn(&serde_json::Value) -> bool,
    ) {
        let key = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap()
            .into_iter()
            .find_map(|(key, raw)| {
                serde_json::from_slice::<serde_json::Value>(&raw)
                    .ok()
                    .filter(|value| predicate(value))
                    .map(|_| key)
            })
            .expect("matching persisted artifact");
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(key, b"corrupt".to_vec(), None).unwrap();
        tx.commit(WriteOptions::sync()).unwrap();
    }

    fn corrupt_fulltext_artifact(cassie: &Cassie, collection: &str, index_name: &str) {
        let prefix = cassie
            .midge
            .fulltext_artifact_prefix_for_diagnostics(collection, index_name)
            .unwrap();
        let key = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .unwrap()
            .into_iter()
            .find_map(|(key, value)| value.starts_with(b"FTM1").then_some(key))
            .expect("full-text manifest artifact");
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(key, b"corrupt".to_vec(), None).unwrap();
        tx.commit(WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_bound_hybrid_reads_to_persisted_text_candidates() {
        // Arrange
        let (cassie, path, collection) = bounded_hybrid_fixture();
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(
            reads < 64,
            "expected bounded hybrid reads, observed {reads}"
        );
        assert!(
            after["hybrid"]["posting_reads_total"].as_u64().unwrap()
                > before["hybrid"]["posting_reads_total"].as_u64().unwrap()
        );
        assert!(
            after["hybrid"]["ann_reads_total"].as_u64().unwrap()
                > before["hybrid"]["ann_reads_total"].as_u64().unwrap()
        );
        assert!(
            after["hybrid"]["exact_reranks_total"].as_u64().unwrap()
                > before["hybrid"]["exact_reranks_total"].as_u64().unwrap()
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_push_structured_filter_over_bounded_hybrid_candidates() {
        // Arrange
        let (cassie, path, collection) = bounded_hybrid_fixture();
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} WHERE status = $3 ORDER BY score DESC LIMIT 1"),
            {
                let mut params = hybrid_params("alpha");
                params.push(Value::String("approved".to_string()));
                params
            },
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(
            reads < 64,
            "expected bounded filtered reads, observed {reads}"
        );
        assert!(
            after["hybrid"]["prefilter_filtered_candidate_count_total"]
                .as_u64()
                .unwrap()
                > before["hybrid"]["prefilter_filtered_candidate_count_total"]
                    .as_u64()
                    .unwrap()
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_when_hybrid_text_artifact_is_corrupt() {
        // Arrange
        let (cassie, path, collection) = bounded_hybrid_fixture();
        corrupt_fulltext_artifact(&cassie, collection, "fulltext_body_idx");
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
        assert_eq!(
            after["hybrid"]["prefilter_fallback_reasons"]["text-artifact"]
                .as_u64()
                .unwrap_or_default()
                - before["hybrid"]["prefilter_fallback_reasons"]["text-artifact"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_when_hybrid_vector_artifact_is_corrupt() {
        // Arrange
        let (cassie, path, collection) = bounded_hybrid_fixture();
        corrupt_matching_data_value(&cassie, |value| {
            value.get("source_fingerprint").is_some()
                && value.get("row_count").is_some()
                && value.get("built_generation").is_some()
        });
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
        assert_eq!(
            after["hybrid"]["prefilter_fallback_reasons"]["vector-artifact"]
                .as_u64()
                .unwrap_or_default()
                - before["hybrid"]["prefilter_fallback_reasons"]["vector-artifact"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bound_hybrid_candidates_before_fulltext_stat_fetch() {
        // Arrange
        const PHYSICAL_ANN_READ_BOUND: u64 = 2 * 64 + 2;
        let (cassie, path, collection) = bounded_hybrid_fixture_with_max(1);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
        assert_eq!(
            after["hybrid"]["candidate_budget_rejections_total"]
                .as_u64()
                .unwrap_or_default()
                - before["hybrid"]["candidate_budget_rejections_total"]
                    .as_u64()
                    .unwrap_or_default(),
            0
        );
        assert_eq!(
            after["hybrid"]["truncation_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before["hybrid"]["truncation_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            0
        );
        let ann_reads = after["hybrid"]["ann_reads_total"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["ann_reads_total"]
                .as_u64()
                .unwrap_or_default();
        let candidates = after["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default();
        assert!(candidates <= 1, "observed {candidates} hybrid candidates");
        assert!(
            ann_reads <= PHYSICAL_ANN_READ_BOUND,
            "observed {ann_reads} controlled ANN reads"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enforce_query_memory_budget_for_hybrid_candidates() {
        // Arrange
        let (cassie, path, collection) = bounded_hybrid_fixture_with_limits(100_000, Some(16));
        let session = cassie.create_session("tester", None);

        // Act
        let error = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .expect_err("hybrid candidates should exceed the query memory budget");

        // Assert
        assert!(
            error.to_string().contains("query memory budget"),
            "unexpected error: {error}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_bounded_hybrid_queries_concurrently() {
        // Arrange
        let (cassie, path, collection) = bounded_hybrid_fixture();
        let cassie = std::sync::Arc::new(cassie);

        // Act
        let handles = (0..4)
        .map(|_| {
            let cassie = std::sync::Arc::clone(&cassie);
            let collection = collection.to_string();
            std::thread::spawn(move || {
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
                        hybrid_params("alpha"),
                    )
                    .unwrap()
                    .rows[0][0]
                    .clone()
            })
        })
        .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(results.len(), 4);
        assert!(results
            .into_iter()
            .all(|value| value == Value::String("d0".to_string())));
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_vector_indexes.rs.
mod integration_sql_vector_indexes {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::rest;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    fn assert_f64_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= f64::EPSILON,
            "expected {actual} to equal {expected}"
        );
    }

    #[test]
    fn should_explain_vector_prefilter_for_indexed_equality_filter() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_vector_prefilter_indexed");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_explain_vector_prefilter_indexed (status TEXT, embedding VECTOR(2), title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_vector_prefilter_status_idx ON sql_explain_vector_prefilter_indexed USING btree (status)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_explain_vector_prefilter_indexed",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "embedding": [1.0, 0.0], "title": "alpha"}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_explain_vector_prefilter_indexed WHERE status = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=index=sql_explain_vector_prefilter_status_idx"));
        assert!(plan.contains("index_aware=true"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_vector_metadata_prefilter_for_supported_predicates() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_prefilter_supported_predicates");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_prefilter_supported_predicates (status TEXT, rating INT, category TEXT, archived_at TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "rating": 5, "category": "alpha", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d2".to_string()),
                serde_json::json!({"status": "approved", "rating": 5, "category": "alpha", "archived_at": null, "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d3".to_string()),
                serde_json::json!({"status": "approved", "rating": 3, "category": "alpha", "archived_at": null, "embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d4".to_string()),
                serde_json::json!({"status": "pending", "rating": 5, "category": "alpha", "archived_at": null, "embedding": [1.0, 0.0]}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_supported_predicates WHERE (status = 'approved') AND (rating BETWEEN 4 AND 6) AND (category IN ('alpha', 'beta')) AND archived_at IS NULL ORDER BY distance ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value == 0.0));
        assert!(matches!(result.rows[1][1], Value::Float64(value) if value == 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fall_back_for_unsupported_vector_metadata_predicate_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_prefilter_unsupported_predicate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_prefilter_unsupported_predicate (status TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_unsupported_predicate",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_unsupported_predicate",
                Some("d2".to_string()),
                serde_json::json!({"status": "pending", "embedding": [0.0, 1.0]}),
            )
            .unwrap();

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_unsupported_predicate WHERE lower(status) = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_unsupported_predicate WHERE lower(status) = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=fallback=unsupported metadata predicate"));
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_explain_hybrid_prefilter_for_indexed_equality_filter() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_hybrid_prefilter_indexed");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_explain_hybrid_prefilter_indexed (status TEXT, body TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_hybrid_prefilter_status_idx ON sql_explain_hybrid_prefilter_indexed USING btree (status)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_explain_hybrid_prefilter_indexed",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "body": "red", "embedding": [1.0, 0.0]}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_explain_hybrid_prefilter_indexed WHERE status = 'approved' ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=index=sql_explain_hybrid_prefilter_status_idx"));
        assert!(plan.contains("index_aware=true"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_vector_index_when_embedding_dimensions_mismatch() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_index_embedding_dimension_mismatch");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_index_embedding_dimension_mismatch (content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        // Act
        let created = cassie
            .execute_sql(
                &session,
                "CREATE INDEX vector_index_embedding_dimension_mismatch_idx ON vector_index_embedding_dimension_mismatch USING vector (embedding) WITH (source_field = content)",
                vec![],
            );

        // Assert
        assert!(created.is_err());
        assert!(created
            .unwrap_err()
            .to_string()
            .contains("embedding dimension mismatch"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_hydrate_hnsw_vector_index_options_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("hnsw_vector_index_options");
        {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_hnsw_vector_index_options (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .unwrap();
            cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_hnsw_vector_index_options_idx ON sql_hnsw_vector_index_options USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 12, ef_construction = 96, ef_search = 48)",
                vec![],
            )
            .unwrap();
        }

        // Act
        let restarted =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        restarted.startup().unwrap();
        let index = restarted
            .catalog
            .get_vector_index("sql_hnsw_vector_index_options", "embedding")
            .expect("hnsw vector index should hydrate");

        // Assert
        assert_eq!(index.metadata.index_type, VectorIndexType::Hnsw);
        let hnsw = index.metadata.hnsw.expect("hnsw options");
        assert_eq!(hnsw.m, 12);
        assert_eq!(hnsw.ef_construction, 96);
        assert_eq!(hnsw.ef_search, 48);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_family_specific_sql_vector_index_options() {
        // Arrange
        use_local_storage();
        let path = data_dir("sql_vector_index_family_options");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE sql_vector_index_family_options (content TEXT, embedding VECTOR(1536))",
            vec![],
        )
        .unwrap();
        let cases = [
        (
            "CREATE INDEX sql_vector_index_family_options_brute_idx ON sql_vector_index_family_options USING vector (embedding) WITH (source_field = content, index_type = bruteforce, m = 12)",
            "vector index option 'm' requires index_type 'hnsw'",
        ),
        (
            "CREATE INDEX sql_vector_index_family_options_hnsw_idx ON sql_vector_index_family_options USING vector (embedding) WITH (source_field = content, index_type = hnsw, lists = 2)",
            "vector index option 'lists' requires index_type 'ivfflat'",
        ),
        (
            "CREATE INDEX sql_vector_index_family_options_ivf_idx ON sql_vector_index_family_options USING vector (embedding) WITH (source_field = content, index_type = ivfflat, ef_search = 64)",
            "vector index option 'ef_search' requires index_type 'hnsw'",
        ),
    ];

        for (sql, expected) in cases {
            // Act
            let error = cassie
                .execute_sql(&session, sql, vec![])
                .expect_err("wrong-family vector option should fail");

            // Assert
            assert!(
                error.to_string().contains(expected),
                "expected '{expected}' in {error}"
            );
        }

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_normalized_vector_sidecars_after_sql_writes() {
        // Arrange
        use_local_storage();
        let path = data_dir("normalized_sidecar_sql_rebuild");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE normalized_sidecar_sql_rebuild (title TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "normalized_sidecar_sql_rebuild");

        let row_id = match &cassie
            .execute_sql(
                &session,
                "INSERT INTO normalized_sidecar_sql_rebuild (title, embedding) VALUES ('alpha', $1) RETURNING _id",
                vec![Value::Vector(Vector::new(vec![3.0, 4.0, 0.0]))],
            )
            .unwrap()
            .rows[0][0]
        {
            Value::String(id) => id.clone(),
            other => panic!("expected string row id, got {other:?}"),
        };
        cassie
            .execute_sql(
                &session,
                "UPDATE normalized_sidecar_sql_rebuild SET embedding = $1 WHERE title = 'alpha'",
                vec![Value::Vector(Vector::new(vec![0.0, 0.0, 5.0]))],
            )
            .unwrap();

        let vector_index = VectorIndexRecord {
            collection: collection.clone(),
            field: "embedding".to_string(),
            source_field: "title".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::Cosine,
                index_type: VectorIndexType::BruteForce,
                hnsw: None,
                hnsw_graph: None,
                ivfflat: None,
                ivfflat_training: None,
            },
        };

        // Act
        cassie.midge.put_vector_index(vector_index.clone()).unwrap();
        let stored = cassie
            .midge
            .get_normalized_vector(&collection, "embedding", &row_id)
            .unwrap()
            .unwrap();

        clear_normalized_sidecars(&cassie, &collection, "embedding");
        assert!(
            cassie
                .midge
                .get_normalized_vector(&collection, "embedding", &row_id)
                .unwrap()
                .is_none()
        );

        cassie
            .midge
            .rebuild_normalized_vectors_for_index(&vector_index)
            .unwrap();
        let rebuilt = cassie
            .midge
            .get_normalized_vector(&collection, "embedding", &row_id)
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(stored.collection, collection);
        assert_eq!(stored.field, "embedding");
        assert_eq!(stored.id, row_id);
        assert_eq!(stored.dimensions, 3);
        assert_eq!(stored.metric, DistanceMetric::Cosine);
        assert!(stored.payload_available);
        assert_eq!(stored.normalization_version, 1);
        assert_eq!(stored.values, vec![0.0, 0.0, 1.0]);
        assert_f64_close(stored.magnitude, 5.0);
        assert_eq!(rebuilt.values, stored.values);
        assert_f64_close(rebuilt.magnitude, stored.magnitude);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    // Merged from tests/rest_vector_indexes.rs to cut a separate test binary.
    #[test]
    fn should_reject_family_specific_rest_vector_index_options() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest_vector_index_family_options");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_vector_index_family_options (content TEXT, embedding VECTOR(1536))",
            vec![],
        )
        .unwrap();
        let cases = [
            (
                serde_json::json!({
                    "kind": "vector",
                    "field": "embedding",
                    "options": {
                        "source_field": "content",
                        "index_type": "bruteforce",
                        "m": "12"
                    }
                }),
                "vector index option 'm' requires index_type 'hnsw'",
            ),
            (
                serde_json::json!({
                    "kind": "vector",
                    "field": "embedding",
                    "options": {
                        "source_field": "content",
                        "index_type": "hnsw",
                        "lists": "2"
                    }
                }),
                "vector index option 'lists' requires index_type 'ivfflat'",
            ),
            (
                serde_json::json!({
                    "kind": "vector",
                    "field": "embedding",
                    "options": {
                        "source_field": "content",
                        "index_type": "ivfflat",
                        "ef_search": "64"
                    }
                }),
                "vector index option 'ef_search' requires index_type 'hnsw'",
            ),
        ];

        for (body, expected) in cases {
            // Act
            let error = rest::indexes::create(
                &cassie,
                "rest_vector_index_family_options",
                body.to_string().as_bytes(),
            )
            .expect_err("wrong-family vector option should fail");

            // Assert
            assert!(
                error.to_string().contains(expected),
                "expected '{expected}' in {error}"
            );
        }

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_vector_query.rs.
mod integration_sql_vector_query {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_project_cosine_distance_for_vector_fields() {
        // Arrange
        use_local_storage();
        let path = data_dir("cosine_distance_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_cosine_distance_projection";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("same".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("orthogonal".to_string()),
                serde_json::json!({"embedding": [0.0, 1.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, cosine_distance(embedding, '[1,0]') AS distance FROM sql_cosine_distance_projection ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("orthogonal".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(1.0));
        assert_eq!(result.rows[1][0], Value::String("same".to_string()));
        assert_eq!(result.rows[1][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_project_dot_product_for_vector_fields() {
        // Arrange
        use_local_storage();
        let path = data_dir("dot_product_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_dot_product_projection";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                }],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"embedding": [1.0, 2.0]}),
                )
                .unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
            .execute_sql(
                &session,
                "SELECT dot_product(embedding, '[3,4]') AS score FROM sql_dot_product_projection",
                vec![],
            )
            .unwrap();

            // Assert
            assert_eq!(result.rows, vec![vec![Value::Float64(11.0)]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_project_l2_distance_for_vector_fields() {
        // Arrange
        use_local_storage();
        let path = data_dir("l2_distance_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_l2_distance_projection";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [4.0, 6.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT vector_distance(embedding, '[1,2]') AS distance FROM sql_l2_distance_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_project_pgvector_operator_distances() {
        // Arrange
        use_local_storage();
        let path = data_dir("pgvector_operator_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_pgvector_operator_projection";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT embedding <-> '[1,0]' AS l2, embedding <=> '[1,0]' AS cosine, embedding <#> '[1,0]' AS dot FROM sql_pgvector_operator_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Float64(1.0),
                Value::Float64(0.0),
                Value::Float64(-2.0)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/ivfflat_completeness.rs.
mod ivfflat_completeness {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::embeddings::{
        DistanceMetric, IvfFlatIndexOptions, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use support::{data_dir, use_local_storage};

    fn seed_ivfflat(cassie: &Cassie, collection: &str) {
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(3),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .expect("create collection");
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        for (id, embedding) in [
            ("near", [1.0, 0.0, 0.0]),
            ("middle", [0.5, 0.5, 0.0]),
            ("far", [-1.0, 0.0, 0.0]),
        ] {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(id.to_string()),
                    serde_json::json!({"content": id, "embedding": embedding}),
                )
                .expect("put vector document");
        }
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "content".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::IvfFlat,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: Some(IvfFlatIndexOptions {
                        version: 1,
                        lists: 2,
                        probes: 2,
                        training_sample_size: 3,
                        training_seed: 17,
                    }),
                    ivfflat_training: None,
                },
            })
            .expect("put IVFFlat index");
    }

    fn remove_one_membership(cassie: &Cassie, collection: &str) {
        let prefix = cassie
            .midge
            .ivfflat_membership_prefix_for_diagnostics(collection, "embedding")
            .expect("membership prefix");
        let key = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("membership scan")
            .into_iter()
            .next()
            .expect("persisted membership")
            .0;
        let mut tx = cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("data transaction");
        tx.delete(key).expect("delete membership");
        tx.commit(WriteOptions::sync())
            .expect("commit membership corruption");
    }

    fn execute_top_k(cassie: &Cassie, collection: &str) -> cassie::executor::QueryResult {
        cassie
            .execute_sql(
                &cassie.create_session("tester", None),
                &format!(
                    "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"
                ),
                vec![],
            )
            .expect("execute exact fallback query")
    }

    fn assert_exact_fallback(cassie: &Cassie, collection: &str, expected_reason: &str) {
        let before = cassie.metrics();
        let result = execute_top_k(cassie, collection);
        let after = cassie.metrics();

        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_eq!(
            after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
                - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
            1
        );
        assert_eq!(
            after["vector"]["last_fallback_reason"].as_str(),
            Some(expected_reason)
        );
    }

    #[test]
    fn should_fallback_exactly_given_missing_membership_in_probed_ivfflat_list() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_missing_probed_membership");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let collection = "ivfflat_missing_probed_membership";
        seed_ivfflat(&cassie, collection);
        remove_one_membership(&cassie, collection);

        // Act
        assert_exact_fallback(&cassie, collection, "stale-list-membership");

        // Assert
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_exactly_given_corrupt_ivfflat_list_count() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_corrupt_list_count");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let collection = "ivfflat_corrupt_list_count";
        seed_ivfflat(&cassie, collection);
        let mut state = cassie
            .midge
            .get_vector_index_state(collection, "embedding")
            .expect("read vector state")
            .expect("persisted vector state");
        state
            .ivfflat_training
            .as_mut()
            .expect("IVFFlat training")
            .list_sizes[0] += 1;
        cassie
            .midge
            .put_vector_index_state(collection, "embedding", state)
            .expect("persist corrupt list count");

        // Act
        assert_exact_fallback(&cassie, collection, "stale-list-sizes");

        // Assert
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_incomplete_ivfflat_membership_on_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_restart_rebuild");
        let collection = "ivfflat_restart_rebuild";
        {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            seed_ivfflat(&cassie, collection);
            remove_one_membership(&cassie, collection);
        }
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");

        // Act
        restarted.startup().expect("repair startup");
        let before = restarted.metrics();
        let result = execute_top_k(&restarted, collection);
        let after = restarted.metrics();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_eq!(
            after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
                - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
            0
        );
        assert_eq!(
            after["vector"]["ivfflat_executions"].as_u64().unwrap()
                - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
            1
        );
        let prefix = restarted
            .midge
            .ivfflat_membership_prefix_for_diagnostics(collection, "embedding")
            .expect("membership prefix");
        let observed_memberships = restarted
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("membership scan")
            .len();
        let (_, expected_memberships) = restarted
            .midge
            .get_ivfflat_training_manifest(collection, "embedding")
            .expect("read manifest")
            .expect("persisted manifest");
        assert_eq!(observed_memberships, expected_memberships);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_unsupported_ivfflat_training_version_on_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_version_rebuild");
        let collection = "ivfflat_version_rebuild";
        {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            seed_ivfflat(&cassie, collection);
            let mut state = cassie
                .midge
                .get_vector_index_state(collection, "embedding")
                .expect("read vector state")
                .expect("persisted vector state");
            state
                .ivfflat_training
                .as_mut()
                .expect("IVFFlat training")
                .version = 0;
            cassie
                .midge
                .put_vector_index_state(collection, "embedding", state)
                .expect("persist old training version");
        }
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");

        // Act
        restarted.startup().expect("repair startup");
        let (training, _) = restarted
            .midge
            .get_ivfflat_training_manifest(collection, "embedding")
            .expect("read repaired manifest")
            .expect("repaired manifest");

        // Assert
        assert_eq!(training.version, 1);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_corrupt_membership_key_when_hydrating_ivfflat_state() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_corrupt_membership_key");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let collection = "ivfflat_corrupt_membership_key";
        seed_ivfflat(&cassie, collection);
        let mut corrupt_key = cassie
            .midge
            .ivfflat_membership_prefix_for_diagnostics(collection, "embedding")
            .expect("membership prefix");
        corrupt_key.extend_from_slice(b"truncated");
        cassie
            .midge
            .raw_put(StorageFamily::Data, &corrupt_key, &[])
            .expect("inject corrupt membership key");

        // Act
        let error = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .expect_err("corrupt membership key must fail hydration");

        // Assert
        assert!(error.to_string().contains("invalid IVFFlat membership key"));
        let _ = std::fs::remove_dir_all(path);
    }
}
// Formerly tests/ivfflat_indexes.rs.
mod ivfflat_indexes {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::IndexKind;
    use cassie::embeddings::{
        DistanceMetric, IvfFlatIndexOptions, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::sql::ast::QueryStatement;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    fn register_ivfflat_collection(cassie: &Cassie, collection: &str) {
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(3),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
    }

    fn put_ivfflat_document(cassie: &Cassie, collection: &str, id: &str, embedding: [f64; 3]) {
        cassie
            .midge
            .put_document(
                collection,
                Some(id.to_string()),
                serde_json::json!({"content": id, "embedding": embedding}),
            )
            .unwrap();
    }

    fn put_ivfflat_index(cassie: &Cassie, collection: &str, seed: u64) {
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "content".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::IvfFlat,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: Some(IvfFlatIndexOptions {
                        version: 1,
                        lists: 2,
                        probes: 1,
                        training_sample_size: 3,
                        training_seed: seed,
                    }),
                    ivfflat_training: None,
                },
            })
            .unwrap();
    }

    fn stored_ivfflat_index(cassie: &Cassie, collection: &str) -> VectorIndexRecord {
        cassie
            .midge
            .get_vector_index(collection, "embedding")
            .unwrap()
            .expect("ivfflat vector index should persist")
    }

    fn mutate_stored_ivfflat_index(
        cassie: &Cassie,
        collection: &str,
        mut mutate: impl FnMut(&mut VectorIndexRecord),
    ) {
        let mut record = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .unwrap()
            .expect("stored vector index metadata should exist");
        mutate(&mut record);
        cassie
            .midge
            .put_vector_index_state(
                collection,
                "embedding",
                cassie::embeddings::VectorIndexState {
                    built_generation: 0,
                    hnsw_graph: record.metadata.hnsw_graph,
                    ivfflat_training: record.metadata.ivfflat_training,
                },
            )
            .unwrap();
    }

    fn ivfflat_row_count(cassie: &Cassie, collection: &str) -> usize {
        stored_ivfflat_index(cassie, collection)
            .metadata
            .ivfflat_training
            .unwrap()
            .row_count
    }

    fn assert_candidate_list_training(stored: VectorIndexRecord) {
        let training = stored
            .metadata
            .ivfflat_training
            .expect("ivfflat training state");
        assert!(training.trained);
        assert_ne!(training.source_fingerprint, 0);
        assert_eq!(training.row_count, 3);
        assert_eq!(training.lists, 2);
        assert_eq!(training.probes, 1);
        assert_eq!(training.assignments.len(), 3);
        assert_eq!(training.list_sizes.iter().sum::<usize>(), 3);
    }

    fn assert_candidate_list_metrics(before: &serde_json::Value, after: &serde_json::Value) {
        let vector_count_delta = after["vector"]["count"].as_u64().unwrap()
            - before["vector"]["count"].as_u64().unwrap();
        let candidate_count_delta = after["vector"]["candidate_count_total"].as_u64().unwrap()
            - before["vector"]["candidate_count_total"].as_u64().unwrap();
        assert_eq!(vector_count_delta, 1);
        assert!(candidate_count_delta < 3);
        assert_eq!(
            after["vector"]["ivfflat_executions"].as_u64().unwrap()
                - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
            1
        );
        assert_eq!(after["vector"]["last_index_kind"].as_str(), Some("ivfflat"));
        assert!(
            after["vector"]["ivfflat_exact_reranks_total"]
                .as_u64()
                .unwrap()
                > before["vector"]["ivfflat_exact_reranks_total"]
                    .as_u64()
                    .unwrap()
        );
    }

    fn assert_ivfflat_fallback_query(
        cassie: &Cassie,
        collection: &str,
        expected_reason: &str,
        before: &serde_json::Value,
    ) {
        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            &format!(
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"
            ),
            vec![],
        )
        .unwrap();
        let after = cassie.metrics();

        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_eq!(
            after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
                - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
            1
        );
        assert_eq!(
            after["vector"]["last_fallback_reason"].as_str(),
            Some(expected_reason)
        );
    }

    #[test]
    fn should_parse_ivfflat_vector_index_options() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_embedding_ivf ON docs USING vector (embedding) WITH (source_field = content, index_type = ivfflat, lists = 16, probes = 4, training_sample_size = 128, training_seed = 42)";

        // Act
        let parsed = cassie::sql::parse_statement(sql).unwrap();

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected CREATE INDEX");
        };
        assert_eq!(statement.kind, IndexKind::Vector);
        assert_eq!(
            statement.options.get("index_type"),
            Some(&"ivfflat".to_string())
        );
        assert_eq!(statement.options.get("lists"), Some(&"16".to_string()));
        assert_eq!(statement.options.get("probes"), Some(&"4".to_string()));
    }

    #[test]
    fn should_persist_ivfflat_vector_index_options() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_vector_index_options");
        let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors())
            .expect("cassie");
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ivfflat_docs (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "ivfflat_docs");

        // Act
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_ivfflat_docs_embedding ON ivfflat_docs USING vector (embedding) WITH (source_field = content, metric = l2, index_type = ivfflat, lists = 8, probes = 3, training_sample_size = 64, training_seed = 99)",
            vec![],
        )
        .unwrap();
        let stored = cassie
            .midge
            .get_vector_index(&collection, "embedding")
            .unwrap()
            .expect("ivfflat vector index should persist");

        // Assert
        assert_eq!(stored.metadata.index_type, VectorIndexType::IvfFlat);
        let ivfflat = stored.metadata.ivfflat.expect("ivfflat options");
        assert_eq!(ivfflat.lists, 8);
        assert_eq!(ivfflat.probes, 3);
        assert_eq!(ivfflat.training_sample_size, 64);
        assert_eq!(ivfflat.training_seed, 99);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_use_trained_ivfflat_candidate_lists_for_top_k() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_candidate_lists");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_candidate_lists";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 7);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let stored = stored_ivfflat_index(&cassie, collection);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM ivfflat_candidate_lists ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_candidate_list_training(stored);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_candidate_list_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_read_only_probed_ivfflat_candidates() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_point_reads");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_point_reads";
        register_ivfflat_collection(&cassie, collection);
        for index in 0..32 {
            let embedding = if index < 16 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            put_ivfflat_document(&cassie, collection, &format!("doc-{index}"), embedding);
        }
        put_ivfflat_index(&cassie, collection, 7);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);
        let relation = cassie
            .catalog
            .get_schema(collection)
            .expect("registered collection schema")
            .collection
            .clone();

        // Act
        cassie
        .execute_sql(
            &session,
            &format!("SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {relation} ORDER BY distance ASC LIMIT 1"),
            vec![],
        )
        .unwrap();
        let after = cassie.metrics();

        // Assert
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(reads < 32, "expected probed-list reads, observed {reads}");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_read_persisted_ivfflat_candidate_ids_without_source_scan() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_persisted_candidate_ids");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_persisted_candidate_ids";
        register_ivfflat_collection(&cassie, collection);
        for index in 0..32 {
            let embedding = if index < 16 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            put_ivfflat_document(&cassie, collection, &format!("doc-{index}"), embedding);
        }
        put_ivfflat_index(&cassie, collection, 31);
        let before = cassie.metrics();

        // Act
        let candidates = cassie
            .midge
            .persisted_vector_candidate_ids(collection, "embedding", &[1.0, 0.0, 0.0], 32)
            .unwrap()
            .expect("persisted ivfflat candidates");
        let after = cassie.metrics();

        // Assert
        assert!(!candidates.is_empty());
        assert!(candidates.len() <= 32);
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(
            reads < 32,
            "expected persisted candidate reads, observed {reads}"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_store_ivfflat_membership_outside_training_manifest() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_membership_layout");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_membership_layout";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);

        // Act
        put_ivfflat_index(&cassie, collection, 17);
        let state_key = cassie
            .midge
            .vector_state_key_for_diagnostics(collection, "embedding")
            .unwrap();
        let state = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &state_key)
            .unwrap();
        let state = state
            .into_iter()
            .find_map(|(key, value)| (key == state_key).then_some(value))
            .expect("persisted ivfflat state");
        let (training, membership_count) = cassie
            .midge
            .get_ivfflat_training_manifest(collection, "embedding")
            .unwrap()
            .expect("ivfflat manifest");

        // Assert
        assert_eq!(state.first(), Some(&3));
        assert_ne!(state.first(), Some(&0x7b));
        assert!(training.assignments.is_empty());
        assert_eq!(membership_count, 2);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_refresh_ivfflat_training_after_document_writes() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_write_refresh");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_write_refresh";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 11);
        assert_eq!(ivfflat_row_count(&cassie, collection), 2);

        // Act
        put_ivfflat_document(&cassie, collection, "new-nearest", [0.9, 0.0, 0.0]);
        let after_insert = stored_ivfflat_index(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[0.9,0,0]') AS distance FROM ivfflat_write_refresh ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .delete_document(collection, "new-nearest")
            .unwrap();
        let after_delete = stored_ivfflat_index(&cassie, collection);

        // Assert
        let inserted_training = after_insert
            .metadata
            .ivfflat_training
            .expect("ivfflat training after insert");
        assert_eq!(inserted_training.row_count, 3);
        assert!(inserted_training.assignments.contains_key("new-nearest"));
        assert_eq!(result.rows[0][0], Value::String("new-nearest".to_string()));
        let deleted_training = after_delete
            .metadata
            .ivfflat_training
            .expect("ivfflat training after delete");
        assert_eq!(deleted_training.row_count, 2);
        assert!(!deleted_training.assignments.contains_key("new-nearest"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_keep_ivfflat_reads_safe_during_concurrent_mutation() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_concurrent_mutation");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_concurrent_mutation";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 41);
        let cassie = std::sync::Arc::new(cassie);

        // Act
        let readers = (0..4)
        .map(|_| {
            let cassie = std::sync::Arc::clone(&cassie);
            std::thread::spawn(move || {
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM ivfflat_concurrent_mutation ORDER BY distance ASC LIMIT 1",
                        vec![],
                    )
                    .unwrap()
                    .rows[0][0]
                    .clone()
            })
        })
        .collect::<Vec<_>>();
        let writer = {
            let cassie = std::sync::Arc::clone(&cassie);
            std::thread::spawn(move || {
                put_ivfflat_document(
                    cassie.as_ref(),
                    "ivfflat_concurrent_mutation",
                    "new-nearest",
                    [0.99, 0.0, 0.0],
                );
            })
        };
        writer.join().unwrap();
        let results = readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(results.len(), 4);
        assert!(results.into_iter().all(|value| {
            value == Value::String("near".to_string())
                || value == Value::String("new-nearest".to_string())
        }));
        assert_eq!(ivfflat_row_count(cassie.as_ref(), collection), 4);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_ivfflat_training_assignment_coverage_is_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_missing_assignment_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_missing_assignment_fallback";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 21);
        mutate_stored_ivfflat_index(&cassie, collection, |record| {
            let training = record
                .metadata
                .ivfflat_training
                .as_mut()
                .expect("ivfflat training");
            training.assignments.remove("far");
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "incomplete-assignments";

        // Assert
        assert_ivfflat_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_ivfflat_training_list_bounds_are_bad() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_bad_list_bounds_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_bad_list_bounds_fallback";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 22);
        mutate_stored_ivfflat_index(&cassie, collection, |record| {
            let training = record
                .metadata
                .ivfflat_training
                .as_mut()
                .expect("ivfflat training");
            training
                .assignments
                .insert("near".to_string(), training.lists);
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "stale-list-membership";

        // Assert
        assert_ivfflat_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fall_back_when_same_row_count_ivfflat_training_fingerprint_is_stale() {
        // Arrange
        use_local_storage();
        let path = data_dir("ivfflat_stale_fingerprint_fallback");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_stale_fingerprint_fallback";
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 23);
        mutate_stored_ivfflat_index(&cassie, collection, |record| {
            let training = record
                .metadata
                .ivfflat_training
                .as_mut()
                .expect("ivfflat training");
            training.source_fingerprint ^= 1;
        });
        let before = cassie.metrics();

        // Act
        let expected_reason = "stale-source-fingerprint";

        // Assert
        assert_ivfflat_fallback_query(&cassie, collection, expected_reason, &before);

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/normalized_vector_generation.rs.
mod normalized_vector_generation {
    use cassie::app::Cassie;
    use cassie::embeddings::{
        DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_reject_normalized_vectors_from_an_older_collection_generation() {
        // Arrange
        use_local_storage();
        let path = data_dir("normalized_vector_generation");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = "normalized_vector_generation_docs";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .expect("create collection");
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
            )
            .expect("insert document");
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "embedding".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::Hnsw,
                    hnsw: Some(HnswIndexOptions::default()),
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            })
            .expect("create vector index");

        // Act
        rewrite_sidecar_generation(&cassie, collection, 0);

        // Assert
        assert!(cassie
            .midge
            .list_normalized_vectors(collection, "embedding")
            .expect("read sidecars")
            .is_empty());

        let _ = std::fs::remove_dir_all(path);
    }

    fn rewrite_sidecar_generation(cassie: &Cassie, collection: &str, generation: u64) {
        let prefix = cassie
            .midge
            .normalized_vector_prefix_for_diagnostics(collection, "embedding")
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("scan data");
        let mut tx = cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("open data transaction");
        for (key, mut raw) in entries {
            assert_eq!(raw.first(), Some(&1), "normalized-vector format marker");
            raw[5..13].copy_from_slice(&generation.to_be_bytes());
            tx.put(key, raw, None).expect("write stale sidecar");
        }
        tx.commit(WriteOptions::sync())
            .expect("commit stale sidecar");
    }
}

// Formerly tests/openai_provider.rs.
mod openai_provider {
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
}

// Formerly tests/vector_index_metadata.rs.
mod vector_index_metadata {
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::embeddings::{
        DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::{TransactionMode, WriteOptions};
    use std::collections::BTreeMap;

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn clear_normalized_sidecars(cassie: &Cassie, collection: &str, field: &str) {
        let prefix = cassie
            .midge
            .normalized_vector_prefix_for_diagnostics(collection, field)
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

    #[test]
    fn should_persist_vector_index_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("persist");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            // Act
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "index_meta_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "content".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(3),
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            let record = VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "content".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "openai".to_string(),
                    model: "text-embedding-3-small".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::Cosine,
                    index_type: VectorIndexType::BruteForce,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            };

            cassie.midge.put_vector_index(record.clone()).unwrap();

            let loaded = cassie
                .midge
                .get_vector_index(collection, "embedding")
                .unwrap()
                .unwrap();

            // Assert
            assert_eq!(loaded, record);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_persist_hnsw_vector_index_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("persist_hnsw");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "hnsw_index_meta_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "content".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(3),
                        nullable: true,
                    },
                ],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            let record = VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "content".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::Hnsw,
                    hnsw: Some(HnswIndexOptions {
                        version: 1,
                        m: 12,
                        ef_construction: 96,
                        ef_search: 48,
                    }),
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            };

            // Act
            cassie.midge.put_vector_index(record.clone()).unwrap();
            let loaded = cassie
                .midge
                .get_vector_index(collection, "embedding")
                .unwrap()
                .unwrap();

            // Assert
            assert_eq!(loaded.collection, record.collection);
            assert_eq!(loaded.field, record.field);
            assert_eq!(loaded.source_field, record.source_field);
            assert_eq!(loaded.metadata.index_type, VectorIndexType::Hnsw);
            assert_eq!(loaded.metadata.hnsw, record.metadata.hnsw);
            let graph = loaded.metadata.hnsw_graph.expect("hnsw graph state");
            assert_eq!(graph.row_count, 0);
            assert_ne!(graph.source_fingerprint, 0);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_reload_registry_after_restart_simulation() {
        // Arrange
        use_local_storage();
        let path = data_dir("restart");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            // Act
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "restart_index_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "text".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "vector".to_string(),
                        data_type: DataType::Vector(2),
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            let record = VectorIndexRecord {
                collection: collection.to_string(),
                field: "vector".to_string(),
                source_field: "text".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "voyage".to_string(),
                    model: "voyage-3-large".to_string(),
                    dimensions: 2,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::BruteForce,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            };

            cassie.midge.put_vector_index(record.clone()).unwrap();
            let before_restart = cassie
                .midge
                .list_vector_indexes()
                .expect("vector indexes before restart");
            assert_eq!(before_restart.len(), 1);
            assert_eq!(before_restart[0], record);

            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();

            let stored = restarted
                .midge
                .list_vector_indexes()
                .expect("stored vector index records");
            assert!(!stored.is_empty());

            let hydrated = restarted
                .catalog
                .get_vector_index(collection, "vector")
                .unwrap();

            // Assert
            assert_eq!(hydrated, record);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_rebuild_missing_normalized_sidecars_on_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("normalized_restart");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            // Act
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "normalized_restart_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(3),
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            let record = VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "body".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::Cosine,
                    index_type: VectorIndexType::BruteForce,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            };
            cassie.midge.put_vector_index(record.clone()).unwrap();

            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({
                        "body": "alpha",
                        "embedding": [3.0, 4.0, 0.0],
                    }),
                )
                .unwrap();

            let stored = cassie
                .midge
                .get_normalized_vector(collection, "embedding", "doc-1")
                .unwrap()
                .unwrap();
            assert_eq!(stored.values, vec![0.6, 0.8, 0.0]);

            clear_normalized_sidecars(&cassie, collection, "embedding");
            assert!(cassie
                .midge
                .get_normalized_vector(collection, "embedding", "doc-1")
                .unwrap()
                .is_none());

            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();

            let rebuilt = restarted
                .midge
                .get_normalized_vector(collection, "embedding", "doc-1")
                .unwrap()
                .unwrap();

            // Assert
            assert_eq!(rebuilt.collection, collection);
            assert_eq!(rebuilt.field, "embedding");
            assert_eq!(rebuilt.id, "doc-1");
            assert_eq!(rebuilt.dimensions, 3);
            assert_eq!(rebuilt.metric, DistanceMetric::Cosine);
            assert!(rebuilt.payload_available);
            assert_eq!(rebuilt.normalization_version, 1);
            assert_eq!(rebuilt.values, vec![0.6, 0.8, 0.0]);
            assert!((rebuilt.magnitude - 5.0).abs() < f64::EPSILON);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_remove_vector_state_when_dropping_vector_field() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_vector_field");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "drop_vector_field_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(3),
                        nullable: true,
                    },
                ],
            };
            cassie.midge.create_collection(collection, schema).unwrap();
            let vector = VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "title".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "openai".to_string(),
                    model: "text-embedding-3-small".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::Cosine,
                    index_type: VectorIndexType::Hnsw,
                    hnsw: Some(HnswIndexOptions::default()),
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            };
            cassie.midge.put_vector_index(vector).unwrap();
            cassie
                .midge
                .put_index(&IndexMeta {
                    collection: collection.to_string(),
                    name: "drop_vector_field_idx".to_string(),
                    field: "embedding".to_string(),
                    fields: vec!["embedding".to_string()],
                    expressions: Vec::new(),
                    include_fields: Vec::new(),
                    predicate: None,
                    kind: IndexKind::Vector,
                    unique: false,
                    options: BTreeMap::new(),
                })
                .unwrap();
            assert!(cassie
                .midge
                .get_vector_index_state(collection, "embedding")
                .unwrap()
                .is_some());

            // Act
            cassie
                .midge
                .alter_collection_drop_column(collection, "embedding")
                .unwrap();

            // Assert
            assert!(cassie
                .midge
                .get_vector_index(collection, "embedding")
                .unwrap()
                .is_none());
            assert!(cassie
                .midge
                .get_vector_index_state(collection, "embedding")
                .unwrap()
                .is_none());
            assert!(cassie
                .midge
                .get_index(collection, "drop_vector_field_idx")
                .unwrap()
                .is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_normalized_sidecar_rebuild_when_index_dimensions_do_not_match_document_values()
    {
        // Arrange
        use_local_storage();
        let path = data_dir("normalized_dimension_mismatch");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            // Act
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "normalized_dimension_mismatch_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(3),
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            let record = VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "embedding".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 4,
                    metric: DistanceMetric::Cosine,
                    index_type: VectorIndexType::BruteForce,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            };
            cassie.midge.put_vector_index(record).unwrap();

            let result = cassie.midge.put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "embedding": [1.0, 2.0, 3.0],
                }),
            );

            // Assert
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("expects 4 dimensions"));
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_reload_generic_index_registry_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("generic_index_restart");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            // Act
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "generic_index_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );

            let record = IndexMeta {
                collection: collection.to_string(),
                name: "idx_generic_title".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: true,
                options: BTreeMap::from_iter(vec![(
                    "case_sensitive".to_string(),
                    "true".to_string(),
                )]),
            };
            cassie.midge.put_index(&record).unwrap();

            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();

            // Assert
            let loaded = restarted
                .catalog
                .get_index(collection, "idx_generic_title")
                .expect("index should hydrate");
            assert_eq!(loaded, record);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_persist_fulltext_index_metadata_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_index_restart");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            // Act
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "fulltext_restart_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );

            let expected = IndexMeta {
                collection: collection.to_string(),
                name: "idx_fulltext_body".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "2".to_string()),
                    ("k1".to_string(), "0.7".to_string()),
                    ("b".to_string(), "0.2".to_string()),
                ]),
            };
            cassie.midge.put_index(&expected).unwrap();

            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();

            // Assert
            let loaded = restarted
                .catalog
                .get_index(collection, "idx_fulltext_body")
                .expect("index should hydrate");
            assert_eq!(loaded, expected);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }
}

// Formerly tests/vector_query_stability.rs.
mod vector_query_stability {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
    use cassie::runtime::QueryCancellationHandle;
    use cassie::types::{Value, Vector};

    use super::support_sql as support;
    use support::*;

    fn vector_cassie(path: &str) -> Cassie {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "deterministic-test".to_string(),
            dimensions: 3,
        });
        Cassie::new_with_data_dir_and_config(path, config).expect("create Cassie")
    }

    fn vector_cassie_with_memory_budget(path: &str, query_memory_budget_bytes: usize) -> Cassie {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "deterministic-test".to_string(),
            dimensions: 3,
        });
        config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        Cassie::new_with_data_dir_and_config(path, config).expect("create Cassie")
    }

    fn seed_vector_collection(cassie: &Cassie, collection: &str, rows: usize) {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!(
                    "CREATE TABLE {collection} (content TEXT, status TEXT, embedding VECTOR(3))"
                ),
                vec![],
            )
            .expect("create vector collection");
        let documents = (0..rows)
            .map(|index| {
                let coordinate = index.to_string().parse::<f64>().expect("f64 index") / 100.0;
                let content = format!("row-{index:04}");
                (
                    Some(content.clone()),
                    serde_json::json!({
                        "content": content,
                        "status": if index % 2 == 0 { "even" } else { "odd" },
                        "embedding": [coordinate, coordinate / 2.0, 0.0]
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(collection, documents)
            .expect("seed vector rows");
    }

    fn vector_query(collection: &str, filter: bool) -> String {
        let predicate = if filter { " WHERE status = $2" } else { "" };
        format!(
        "SELECT id, vector_distance(embedding, $1) AS distance FROM {collection}{predicate} ORDER BY distance ASC LIMIT 10"
    )
    }

    fn query_params(as_vector: bool, filter: bool) -> Vec<Value> {
        let query = if as_vector {
            Value::Vector(Vector::new(vec![0.0, 0.0, 0.0]))
        } else {
            Value::String("[0,0,0]".to_string())
        };
        let mut params = vec![query];
        if filter {
            params.push(Value::String("even".to_string()));
        }
        params
    }

    fn result_ids(rows: &[Vec<Value>]) -> Vec<String> {
        rows.iter()
            .map(|row| row[0].as_str().expect("row id").to_string())
            .collect()
    }

    #[test]
    fn should_match_exact_top_k_with_hnsw_for_bound_vector_parameters() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("vector_hnsw_exact_baseline");
        let cassie = vector_cassie(&path);
        let session = cassie.create_session("tester", None);
        let collection = "vector_hnsw_exact_baseline";
        seed_vector_collection(&cassie, collection, 200);
        let sql = vector_query(collection, false);
        let exact = cassie
            .execute_sql(&session, &sql, query_params(true, false))
            .expect("exact vector query");
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_hnsw_exact_idx ON vector_hnsw_exact_baseline USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 12, ef_construction = 96, ef_search = 64)",
            vec![],
        )
        .expect("create HNSW index");
        let before = cassie.metrics();

        // Act
        let indexed = cassie
            .execute_sql(&session, &sql, query_params(false, false))
            .expect("indexed HNSW query");
        let after = cassie.metrics();

        // Assert
        assert_eq!(indexed.rows, exact.rows);
        assert!(
            after["vector"]["hnsw_executions"].as_u64().unwrap()
                > before["vector"]["hnsw_executions"].as_u64().unwrap()
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reach_ivfflat_recall_threshold_against_exact_top_k() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("vector_ivfflat_recall_baseline");
        let cassie = vector_cassie(&path);
        let session = cassie.create_session("tester", None);
        let collection = "vector_ivfflat_recall_baseline";
        seed_vector_collection(&cassie, collection, 1_000);
        let sql = vector_query(collection, false);
        let exact = cassie
            .execute_sql(&session, &sql, query_params(true, false))
            .expect("exact vector query");
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_ivfflat_recall_idx ON vector_ivfflat_recall_baseline USING vector (embedding) WITH (source_field = content, metric = l2, index_type = ivfflat, lists = 16, probes = 8, training_sample_size = 1000, training_seed = 17)",
            vec![],
        )
        .expect("create IVFFlat index");

        // Act
        let indexed = cassie
            .execute_sql(&session, &sql, query_params(false, false))
            .expect("indexed IVFFlat query");

        // Assert
        let exact_ids = result_ids(&exact.rows).into_iter().collect::<HashSet<_>>();
        let overlap = result_ids(&indexed.rows)
            .into_iter()
            .filter(|id| exact_ids.contains(id))
            .count();
        assert!(
            overlap >= 9,
            "expected recall@10 >= 0.90, overlap={overlap}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_use_explicit_exact_fallback_for_filtered_hnsw_after_delete() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("vector_hnsw_filtered_fallback");
        let cassie = vector_cassie(&path);
        let session = cassie.create_session("tester", None);
        let collection = "vector_hnsw_filtered_fallback";
        seed_vector_collection(&cassie, collection, 100);
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_hnsw_filtered_idx ON vector_hnsw_filtered_fallback USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 8, ef_construction = 64, ef_search = 32)",
            vec![],
        )
        .expect("create HNSW index");
        cassie
            .execute_sql(
                &session,
                "DELETE FROM vector_hnsw_filtered_fallback WHERE content = $1",
                vec![Value::String("row-0000".to_string())],
            )
            .expect("delete nearest row");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &vector_query(collection, true),
                query_params(true, true),
            )
            .expect("filtered exact fallback query");
        let after = cassie.metrics();

        // Assert
        assert!(!result_ids(&result.rows).contains(&"row-0000".to_string()));
        assert_eq!(
            after["vector"]["last_fallback_reason"].as_str(),
            Some("structured-filter-exact")
        );
        assert!(
            after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
                > before["vector"]["hnsw_fallbacks"].as_u64().unwrap()
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enforce_query_memory_budget_during_exact_vector_top_k() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("vector_exact_memory_budget");
        let cassie = vector_cassie_with_memory_budget(&path, 32);
        let session = cassie.create_session("tester", None);
        let collection = "vector_exact_memory_budget";
        seed_vector_collection(&cassie, collection, 20);

        // Act
        let error = cassie
            .execute_sql(
                &session,
                &vector_query(collection, false),
                query_params(true, false),
            )
            .expect_err("exact vector heap should exceed the memory budget");

        // Assert
        assert!(
            error.to_string().contains("query memory budget"),
            "unexpected error: {error}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_during_exact_vector_scoring() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("vector_exact_active_cancellation");
        let cassie = Arc::new(vector_cassie(&path));
        let collection = "vector_exact_active_cancellation";
        seed_vector_collection(&cassie, collection, 100_000);
        let cancellation = QueryCancellationHandle::new();
        let query_cancellation = cancellation.clone();
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            let session = query_cassie.create_session("tester", None);
            query_cassie.execute_sql_with_cancellation(
                &session,
                &vector_query(collection, false),
                query_params(true, false),
                &query_cancellation,
            )
        });
        std::thread::sleep(Duration::from_millis(5));

        // Act
        cancellation.cancel();
        let error = query
            .join()
            .expect("query thread")
            .expect_err("active vector query should be cancelled");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/vector_state_generation.rs.
mod vector_state_generation {
    use cassie::app::Cassie;
    use cassie::embeddings::VectorIndexState;

    use super::support_sql as support;
    use support::{canonical_test_collection, data_dir, use_local_storage};

    #[test]
    fn should_reject_vector_state_from_an_older_collection_generation() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_state_generation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE vector_state_docs (title TEXT, embedding VECTOR(3))",
                    vec![],
                )
                .expect("create table");
            let collection = canonical_test_collection(&cassie, "vector_state_docs");
            cassie
                .midge
                .put_vector_index_state(&collection, "embedding", VectorIndexState::default())
                .expect("store current state");

            // Act
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO vector_state_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .expect("advance collection generation");

            // Assert
            assert!(cassie
                .midge
                .get_vector_index_state(&collection, "embedding")
                .expect("read state")
                .is_none());
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/embedding_support_contract.rs.
mod embedding_support_contract {
    use std::fs;

    #[test]
    fn should_publish_provider_specific_embedding_support_contracts() {
        // Arrange
        let feature_support = fs::read_to_string("docs/feature-support.md")
            .expect("read feature support documentation");
        let readiness = fs::read_to_string("docs/production-readiness.md")
            .expect("read production readiness documentation");
        let evidence = fs::read_to_string("docs/promotion-evidence-matrix.md")
            .expect("read promotion evidence matrix");
        let environment = fs::read_to_string("docs/environment-variables.md")
            .expect("read environment-variable documentation");

        // Act
        let remote_providers = [
            "OpenAI",
            "OpenAI-compatible",
            "TEI",
            "Ollama",
            "Voyage",
            "Cohere",
        ];

        // Assert
        for provider in remote_providers {
            assert!(
                feature_support.contains(&format!("| {provider} embeddings |")),
                "missing provider-specific support row for {provider}"
            );
        }
        assert!(feature_support.contains("| Local deterministic embeddings |"));
        assert!(readiness.contains("HTTP 429"));
        assert!(readiness.contains("mock-provider evidence"));
        assert!(readiness.contains("does not establish hosted availability"));
        assert!(evidence.contains("Provider-specific status contradiction resolved"));
        assert!(evidence.contains("Mock-provider auth and HTTP 429 evidence retained"));
        assert!(environment.contains("CASSIE_OPENAI_MAX_RETRIES"));
        assert!(environment.contains("CASSIE_EMBEDDINGS_MAX_RETRIES"));
        assert!(environment.contains("CASSIE_TEI_MAX_RETRIES"));
        assert!(environment.contains("CASSIE_OLLAMA_MAX_RETRIES"));
        assert!(environment.contains("CASSIE_VOYAGE_MAX_RETRIES"));
        assert!(environment.contains("CASSIE_COHERE_MAX_RETRIES"));
        assert!(environment.contains("does not infer a hosted"));
    }
}
