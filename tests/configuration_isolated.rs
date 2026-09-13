// Consolidated integration suite: configuration_isolated.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/environment.rs"]
mod support_environment;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/sprint01_runtime_baseline.rs.
mod sprint01_runtime_baseline {
    use cassie::catalog::canonical_relation_name;
    use cassie::config::EmbeddingsRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use cassie::{app::CassieError, Cassie, CassieRuntimeConfig};
    use std::env;
    use std::path::PathBuf;

    use super::support_sql as support;
    use support::*;

    use super::support_environment as environment;
    use environment::EnvironmentGuard;

    fn without_fallback() {
        env::remove_var("CASSIE_STORAGE_MODE");
    }

    #[test]
    fn should_startup_be_idempotent_without_state_corruption() {
        // Arrange
        without_fallback();
        let path = data_dir("idempotent_no_corruption");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = canonical_relation_name("postgres", "public", "runtime_docs");
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
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
                .create_collection(&collection, schema.clone())
                .unwrap();
            let _ = cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha", "body": "first"}),
                )
                .unwrap();

            cassie.startup().unwrap();
            let baseline_collections = cassie.catalog.list_collections();
            let baseline_docs = cassie.midge.scan_documents(&collection).unwrap();
            let baseline_layout = cassie.midge.ensure_families_ready().unwrap().clone();

            // Act
            cassie.startup().unwrap();
            let after_collections = cassie.catalog.list_collections();
            let after_docs = cassie.midge.scan_documents(&collection).unwrap();
            let after_layout = cassie.midge.ensure_families_ready().unwrap().clone();

            // Assert
            assert_eq!(baseline_layout.schema.id(), after_layout.schema.id());
            assert_eq!(baseline_layout.data.id(), after_layout.data.id());
            assert_eq!(baseline_layout.temp.id(), after_layout.temp.id());
            assert_eq!(baseline_collections.len(), after_collections.len());
            assert_eq!(baseline_docs.len(), after_docs.len());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_map_storage_bootstrap_failure_to_cassie_error() {
        // Arrange
        without_fallback();
        let base_path = PathBuf::from(data_dir("invalid_bootstrap"));
        let marker = base_path.join("marker");
        let _ = std::fs::create_dir_all(&base_path);
        let _ = std::fs::write(&marker, "locked");
        let path = format!("{}/child", marker.to_string_lossy());

        // Act
        let created = Cassie::new_with_data_dir(&path);

        // Assert
        assert!(matches!(
            created,
            Err(CassieError::Storage(_)
                | CassieError::StorageBootstrap(_)
                | CassieError::StorageMissingFamily(_)
                | CassieError::StorageRetryable(_))
        ));

        let _ = std::fs::remove_file(marker);
    }

    #[test]
    fn should_startup_not_create_side_effects_in_default_family() {
        // Arrange
        let path = data_dir("default_family_side_effects");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "runtime_default_guard";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie.midge.create_collection(collection, schema).unwrap();
            let _ = cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-default-guard".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            cassie.startup().unwrap();

            // Act
            let default_entries = cassie.midge.raw_scan_prefix_named("default", b"").unwrap();

            // Assert
            for (key, _) in default_entries {
                let key = String::from_utf8_lossy(&key);
                assert!(
                    !key.starts_with("__cassie__/"),
                    "no cassie-managed keys should be stored in default family"
                );
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_health_after_startup_reports_ready_state() {
        // Arrange
        let path = data_dir("startup_health");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();

            let before = cassie.health();
            assert_eq!(before["ready"].as_bool(), Some(false));

            cassie.startup().unwrap();

            // Act
            let after = cassie.health();

            // Assert
            assert_eq!(after["ready"].as_bool(), Some(true));
            assert_eq!(after["status"], "ok");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_clear_ready_state_after_shutdown() {
        // Arrange
        let path = data_dir("shutdown_state");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            // Act
            cassie.shutdown();
            let after_health = cassie.health();
            let after_metrics = cassie.metrics();

            // Assert
            assert_eq!(after_health["ready"].as_bool(), Some(false));
            assert_eq!(after_health["status"], "starting");
            assert_eq!(after_metrics["runtime"]["started"].as_bool(), Some(false));
            assert_eq!(after_metrics["runtime"]["shutdown_total"].as_u64(), Some(1));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_startup_respects_runtime_config_defaults() {
        // Arrange
        let keys = [
            "CASSIE_PGWIRE_LISTEN",
            "CASSIE_REST_LISTEN",
            "CASSIE_DEFAULT_DATABASE",
            "CASSIE_ROOT_PASSWORD",
            "CASSIE_EMBEDDINGS_PROVIDER",
        ];
        let _environment = EnvironmentGuard::unset(&keys);

        // Act
        let config = CassieRuntimeConfig::from_env().expect("runtime config");

        // Assert
        assert_eq!(config.pgwire_listen, "127.0.0.1:5432");
        assert_eq!(config.rest_listen, "127.0.0.1:8080");
        assert_eq!(config.password, "postgres");
        assert_eq!(config.password, "postgres");
        assert!(matches!(
            config.embeddings,
            EmbeddingsRuntimeConfig::Disabled
        ));
    }

    #[test]
    fn should_create_session_without_mutating_runtime_state() {
        // Arrange
        let path = data_dir("session_immutability");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let before_health = cassie.health();
            let before_collections = cassie.catalog.list_collections().len();

            // Act
            let session = cassie.create_session("tester", Some("postgres".to_string()));
            let after_health = cassie.health();
            let after_collections = cassie.catalog.list_collections().len();

            // Assert
            assert_eq!(session.user, "tester");
            assert_eq!(session.database, Some("postgres".to_string()));
            assert_eq!(
                before_health["ready"].as_bool(),
                after_health["ready"].as_bool()
            );
            assert_eq!(before_health["status"], after_health["status"]);
            assert_eq!(before_collections, after_collections);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/transport_boundaries.rs.
mod transport_boundaries {
    use super::support_environment as environment;
    use super::support_pgwire as pgwire_support;

    use environment::EnvironmentGuard;

    use std::net::SocketAddr;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{
        data_dir, password_message, read_until_ready, read_wire_frame, simple_query_frame,
        startup_frame, use_local_storage,
    };

    type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
    type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
    type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

    async fn read_auth_frame(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> (u8, Vec<u8>) {
        read_wire_frame(reader).await
    }

    fn read_boundary_counter(
        metrics: &serde_json::Value,
        interface: &str,
        kind: &str,
        op: &str,
    ) -> u64 {
        metrics[interface][kind][op].as_u64().unwrap_or_default()
    }

    fn seed_transport_boundary_docs(cassie: &Cassie) {
        let collection = canonical_relation_name("postgres", "public", "transport_boundary_docs");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .unwrap();
        cassie.register_collection(&collection, schema);
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
    }

    async fn spawn_pgwire_boundary_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, server)
    }

    async fn start_pgwire_session(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
        tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("root", "postgres"))
            .await
            .expect("startup write");
        let auth_frame = read_auth_frame(reader).await;
        if auth_request_code(&auth_frame.1) == Some(3) {
            let password =
                std::env::var("CASSIE_ROOT_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
            tokio::io::AsyncWriteExt::write_all(writer, &password_message(&password))
                .await
                .expect("password write");
            tokio::io::AsyncWriteExt::flush(writer)
                .await
                .expect("flush password");
            let auth_ok = read_auth_frame(reader).await;
            assert_eq!(auth_request_code(&auth_ok.1), Some(0));
        }
        let _ready = read_until_ready(reader).await;
    }

    async fn run_pgwire_boundary_query(addr: SocketAddr) {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_pgwire_session(&mut reader, &mut write_half).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT title FROM transport_boundary_docs ORDER BY title"),
        )
        .await
        .expect("simple query write");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");

        loop {
            let frame = read_wire_frame(&mut reader).await;
            if frame.0 == b'Z' {
                break;
            }
        }
    }

    fn assert_pgwire_boundary_metrics(metrics: &serde_json::Value) {
        let started = read_boundary_counter(
            metrics,
            "pgwire",
            "blocking_started_total",
            "pgwire_simple_query",
        );
        let completed = read_boundary_counter(
            metrics,
            "pgwire",
            "blocking_completed_total",
            "pgwire_simple_query",
        );
        let errors = read_boundary_counter(
            metrics,
            "pgwire",
            "blocking_error_total",
            "pgwire_simple_query",
        );
        let join_failed = read_boundary_counter(
            metrics,
            "pgwire",
            "blocking_join_failed_total",
            "pgwire_simple_query",
        );
        assert_eq!(
            metrics["pgwire"]["simple_queries_total"]
                .as_u64()
                .unwrap_or_default(),
            1
        );
        assert_eq!(started, 1);
        assert_eq!(completed, 1);
        assert_eq!(errors, 0);
        assert_eq!(join_failed, 0);
        assert!(
            metrics["pgwire"]["blocking_elapsed_ms_total"]
                .get("pgwire_simple_query")
                .is_some(),
            "elapsed metric should be present"
        );
    }

    fn auth_request_code(payload: &[u8]) -> Option<i32> {
        payload
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(i32::from_be_bytes)
    }

    #[test]
    fn should_record_pgwire_blocking_boundary_metrics_for_simple_query() {
        // Arrange
        let _environment = EnvironmentGuard::set("CASSIE_ROOT_PASSWORD", "route-password");
        use_local_storage();
        let path = data_dir("pgwire-simple");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            seed_transport_boundary_docs(&cassie);
            let (addr, server) = spawn_pgwire_boundary_server(&cassie).await;

            // Act
            run_pgwire_boundary_query(addr).await;

            // Assert
            let metrics = cassie.metrics();
            assert_pgwire_boundary_metrics(&metrics);

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_record_rest_blocking_route_metrics_for_non_public_routes() {
        // Arrange
        let _environment = EnvironmentGuard::set("CASSIE_ROOT_PASSWORD", "route-password");
        use_local_storage();
        let path = data_dir("rest-route");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let client = reqwest::Client::new();
            let admin_cookie = client
                .post(format!("http://{addr}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "route-password"
                }))
                .send()
                .await
                .expect("login request")
                .headers()
                .get("set-cookie")
                .expect("session cookie")
                .to_str()
                .expect("session cookie value")
                .split(';')
                .next()
                .expect("session cookie pair")
                .to_string();
            let before = cassie.metrics();
            let before_started =
                read_boundary_counter(&before, "rest", "blocking_started_total", "rest_route");
            let before_completed =
                read_boundary_counter(&before, "rest", "blocking_completed_total", "rest_route");

            // Act
            let create_payload = serde_json::json!({
                "name": "boundary_rest_docs",
                "fields": [{"name": "title", "type": "text"}]
            });
            let create = client
                .post(format!("http://{addr}/api/v1/collections"))
                .header("content-type", "application/json")
                .header("cookie", &admin_cookie)
                .body(create_payload.to_string())
                .send()
                .await
                .expect("create request");
            assert_eq!(create.status(), reqwest::StatusCode::OK);

            let list = client
                .get(format!("http://{addr}/api/v1/collections"))
                .header("cookie", &admin_cookie)
                .send()
                .await
                .expect("list request");
            assert_eq!(list.status(), reqwest::StatusCode::OK);

            // Assert
            let metrics = cassie.metrics();
            let started =
                read_boundary_counter(&metrics, "rest", "blocking_started_total", "rest_route");
            let completed =
                read_boundary_counter(&metrics, "rest", "blocking_completed_total", "rest_route");
            let errors =
                read_boundary_counter(&metrics, "rest", "blocking_error_total", "rest_route");
            let join_failed =
                read_boundary_counter(&metrics, "rest", "blocking_join_failed_total", "rest_route");
            let has_latency = metrics["rest"]["blocking_elapsed_ms_total"]
                .get("rest_route")
                .is_some();
            let requests = metrics["rest"]["requests_total"]
                .as_u64()
                .unwrap_or_default();

            assert!(
                requests >= 2,
                "non-public route requests should be recorded"
            );
            assert_eq!(
                started - before_started,
                2,
                "route calls should use boundary helper"
            );
            assert_eq!(
                completed - before_completed,
                2,
                "route calls should complete through boundary helper"
            );
            assert_eq!(errors, 0, "successful route calls should not error");
            assert_eq!(
                join_failed, 0,
                "blocking join should not fail for in-memory execution"
            );
            assert!(has_latency, "elapsed metric should be present");

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    fn assert_offloaded_calls(
        file: &str,
        source: &str,
        helper: &str,
        forbidden: &[&str],
        context_lines: usize,
    ) {
        let lines: Vec<&str> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            for forbidden_call in forbidden {
                if !line.contains(forbidden_call) {
                    continue;
                }

                let start = index.saturating_sub(context_lines);
                let allowed = (start..=index).any(|candidate| lines[candidate].contains(helper));
                assert!(
                allowed,
                "found direct blocking call '{forbidden_call}' in {file} outside {helper}: {line}"
            );
            }
        }
    }

    #[test]
    fn should_forbid_direct_async_transport_calls_without_blocking_helpers() {
        // Arrange
        let pgwire_source = include_str!("../src/pgwire/connection.rs");
        let pgwire_copy_source = include_str!("../src/pgwire/connection/copy.rs");
        let pgwire_extended_source = include_str!("../src/pgwire/connection/extended.rs");
        let rest_source = include_str!("../src/rest/router.rs");

        // Act
        assert_offloaded_calls(
            "src/pgwire/connection.rs",
            pgwire_source,
            "run_pgwire_blocking",
            &[
                "cassie.authenticate_role",
                "cassie.execute_sql",
                "cassie.describe_parsed_statement",
                "cassie.execute_parsed_sql_with_mode",
            ],
            10,
        );
        assert_offloaded_calls(
            "src/pgwire/connection/extended.rs",
            pgwire_extended_source,
            "run_pgwire_blocking",
            &[
                "cassie.describe_parsed_statement",
                "cassie.execute_parsed_sql_with_mode",
            ],
            10,
        );
        assert_offloaded_calls(
            "src/pgwire/connection/copy.rs",
            pgwire_copy_source,
            "run_pgwire_blocking",
            &[
                "crate::sql::parser::parse_statement",
                "crate::sql::binder::bind",
                "cassie.copy_from_csv_stdin",
            ],
            10,
        );
        assert_offloaded_calls(
            "src/rest/router.rs",
            rest_source,
            "run_rest_blocking",
            &[
                "crate::rest::collections::list",
                "crate::rest::collections::create",
                "crate::rest::documents::create",
                "crate::rest::documents::get",
                "crate::rest::documents::delete",
                "crate::rest::indexes::create",
                "crate::rest::search::vector_search",
                "cassie.authenticate_role",
                "cassie.lookup_role",
            ],
            10,
        );
        // Assert
    }
}

// Formerly tests/embedding_provider_controls.rs.
mod embedding_provider_controls {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use cassie::config::CassieRuntimeLimits;
    use cassie::embeddings::cohere::{CohereProvider, CohereProviderConfig};
    use cassie::embeddings::compatible::{
        OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
    };
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
}
