use super::*;
use http_body_util::Full;

struct OversizedBody {
    declared_length: bool,
    next_frame: u8,
    polls: Arc<std::sync::atomic::AtomicUsize>,
}

impl Body for OversizedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        this.polls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let frame = match this.next_frame {
            0 => Some(Ok(hyper::body::Frame::data(Bytes::from(vec![
                0;
                MAX_REST_BODY_BYTES
            ])))),
            1 => Some(Ok(hyper::body::Frame::data(Bytes::from_static(b"x")))),
            _ => None,
        };
        this.next_frame = this.next_frame.saturating_add(1);
        std::task::Poll::Ready(frame)
    }

    fn is_end_stream(&self) -> bool {
        self.next_frame > 1
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        if self.declared_length {
            let remaining = match self.next_frame {
                0 => MAX_REST_BODY_BYTES + 1,
                1 => 1,
                _ => 0,
            };
            hyper::body::SizeHint::with_exact(u64::try_from(remaining).expect("body fits u64"))
        } else {
            hyper::body::SizeHint::default()
        }
    }
}

#[test]
fn should_drain_declared_oversized_body_before_rejecting_with_413() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let data_dir = std::env::temp_dir().join(format!(
        "cassie-router-declared-oversized-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Arc::new(
        Cassie::new_with_data_dir_and_config(
            &data_dir,
            crate::config::CassieRuntimeConfig::default(),
        )
        .expect("cassie"),
    );
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let request = Request::post("/api/v1/auth/login")
        .body(OversizedBody {
            declared_length: true,
            next_frame: 0,
            polls: Arc::clone(&polls),
        })
        .expect("request");

    // Act
    let result = runtime.block_on(read_request_body(
        request,
        &cassie,
        &Method::POST,
        "/api/v1/auth/login",
        Instant::now(),
    ));

    // Assert
    assert!(matches!(result, Err((StatusCode::PAYLOAD_TOO_LARGE, _))));
    assert_eq!(polls.load(std::sync::atomic::Ordering::Relaxed), 3);
    drop(cassie);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn should_drain_streamed_oversized_body_before_rejecting() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let body = OversizedBody {
        declared_length: false,
        next_frame: 0,
        polls: Arc::clone(&polls),
    };

    // Act
    let result = runtime.block_on(collect_request_body(body, Duration::from_secs(1)));

    // Assert
    assert!(matches!(result, Err(RestBodyReadError::TooLarge)));
    assert_eq!(polls.load(std::sync::atomic::Ordering::Relaxed), 3);
}

#[test]
fn should_collect_rest_body_with_an_idle_deadline() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let body = Full::from(Bytes::from_static(b"{}"));

        // Act
        let result = collect_request_body(body, Duration::from_secs(1)).await;

        // Assert
        assert_eq!(result.expect("body collection"), Bytes::from_static(b"{}"));
    });
}

#[test]
fn should_return_owned_numeric_error_response_through_public_route_request() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Arc::new(
            Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
                .expect("cassie"),
        );
        cassie.startup().expect("startup");
        let request_body = serde_json::json!({
            "database": cassie.default_database(),
            "sql": "SELECT CAST('not_numeric' AS NUMERIC)"
        })
        .to_string();
        let (route_result_sender, mut route_result_receiver) =
            tokio::sync::mpsc::unbounded_channel();
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let service = hyper::service::service_fn(move |request: Request<Incoming>| {
            let cassie = Arc::clone(&cassie);
            let route_result_sender = route_result_sender.clone();
            async move {
                let result = route_request(request, cassie).await;
                route_result_sender
                    .send(result.is_ok())
                    .expect("route result receiver");
                let response = match result {
                    Ok(response) => response,
                    Err((status, message)) => {
                        json_response(status, &serde_json::json!({"error": message}))
                    }
                };
                Ok::<_, Infallible>(response)
            }
        });
        let server = tokio::spawn(async move {
            hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(server_io), service)
                .await
                .expect("server connection");
        });
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(client_io))
                .await
                .expect("client handshake");
        let client = tokio::spawn(async move {
            connection.await.expect("client connection");
        });
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/admin/query/execute")
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(request_body)))
            .expect("request");

        // Act
        let response = sender.send_request(request).await.expect("response");
        let direct_route_response = route_result_receiver.recv().await.expect("route result");
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes();
        drop(sender);
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("server shutdown")
            .expect("server task");
        tokio::time::timeout(Duration::from_secs(1), client)
            .await
            .expect("client shutdown")
            .expect("client task");

        // Assert
        assert!(direct_route_response);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let value: serde_json::Value = serde_json::from_slice(&body).expect("error body");
        assert_eq!(
            value["error"],
            "invalid input syntax for type numeric: \"not_numeric\""
        );
    });
}

#[test]
fn should_percent_decode_a_quoted_collection_segment_through_the_http_router() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Arc::new(
            Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
                .expect("cassie"),
        );
        cassie.startup().expect("startup");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE \"Cities\" (id BIGINT PRIMARY KEY, name TEXT)",
                Vec::new(),
            )
            .expect("create quoted table");
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server_cassie = Arc::clone(&cassie);
        let service = hyper::service::service_fn(move |request: Request<Incoming>| {
            let cassie = Arc::clone(&server_cassie);
            async move {
                let response = match route_request(request, cassie).await {
                    Ok(response) => response,
                    Err((status, message)) => {
                        json_response(status, &serde_json::json!({"error": message}))
                    }
                };
                Ok::<_, Infallible>(response)
            }
        });
        let server = tokio::spawn(async move {
            hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(server_io), service)
                .await
                .expect("server connection");
        });
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(client_io))
                .await
                .expect("client handshake");
        let client = tokio::spawn(async move {
            connection.await.expect("client connection");
        });
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/collections/%22Cities%22/documents")
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from_static(
                br#"{"id":1,"name":"quoted"}"#,
            )))
            .expect("request");

        // Act
        let response = sender.send_request(request).await.expect("response");

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        let documents = cassie
            .midge
            .scan_documents("postgres.public.\"Cities\"")
            .expect("scan quoted collection");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].payload["name"], "quoted");
        drop(sender);
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("server deadline")
            .expect("server task");
        client.await.expect("client task");
    });
}

type RouterTestSender = hyper::client::conn::http1::SendRequest<Full<Bytes>>;

struct RouterTestHarness {
    sender: RouterTestSender,
    server: tokio::task::JoinHandle<()>,
    client: tokio::task::JoinHandle<()>,
}

async fn start_router_test_harness(cassie: Arc<Cassie>) -> RouterTestHarness {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let service = hyper::service::service_fn(move |request: Request<Incoming>| {
        let cassie = Arc::clone(&cassie);
        async move {
            let response = match route_request(request, cassie).await {
                Ok(response) => response,
                Err((status, message)) => {
                    json_response(status, &serde_json::json!({"error": message}))
                }
            };
            Ok::<_, Infallible>(response)
        }
    });
    let server = tokio::spawn(async move {
        hyper::server::conn::http1::Builder::new()
            .serve_connection(hyper_util::rt::TokioIo::new(server_io), service)
            .await
            .expect("server connection");
    });
    let (sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(client_io))
            .await
            .expect("client handshake");
    let client = tokio::spawn(async move {
        connection.await.expect("client connection");
    });
    RouterTestHarness {
        sender,
        server,
        client,
    }
}

async fn post_router_document(
    sender: &mut RouterTestSender,
    path: &str,
    document: &'static [u8],
) -> StatusCode {
    let request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from_static(document)))
        .expect("document request");
    let response = sender
        .send_request(request)
        .await
        .expect("document response");
    let status = response.status();
    response
        .into_body()
        .collect()
        .await
        .expect("document response body");
    status
}

async fn stop_router_test_harness(harness: RouterTestHarness) {
    drop(harness.sender);
    tokio::time::timeout(Duration::from_secs(1), harness.server)
        .await
        .expect("server deadline")
        .expect("server task");
    harness.client.await.expect("client task");
}

#[test]
fn should_resolve_quoted_mixed_case_collections_exactly_through_the_http_router() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
            .expect("cassie"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE \"Cities\" (id BIGINT PRIMARY KEY, name TEXT)",
            Vec::new(),
        )
        .expect("create quoted table");

    runtime.block_on(async {
        let mut harness = start_router_test_harness(Arc::clone(&cassie)).await;

        // Act
        let fuzzy_status = post_router_document(
            &mut harness.sender,
            "/api/v1/collections/cities/documents",
            br#"{"id":1,"name":"wrong"}"#,
        )
        .await;
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cities (id BIGINT PRIMARY KEY, name TEXT)",
                Vec::new(),
            )
            .expect("create lowercase table");
        let quoted_status = post_router_document(
            &mut harness.sender,
            "/api/v1/collections/%22Cities%22/documents",
            br#"{"id":2,"name":"quoted"}"#,
        )
        .await;
        let lowercase_status = post_router_document(
            &mut harness.sender,
            "/api/v1/collections/cities/documents",
            br#"{"id":3,"name":"lowercase"}"#,
        )
        .await;

        // Assert
        assert_eq!(fuzzy_status, StatusCode::NOT_FOUND);
        assert_eq!(quoted_status, StatusCode::OK);
        assert_eq!(lowercase_status, StatusCode::OK);
        for (collection, expected_rows) in [
            ("postgres.public.\"Cities\"", 1),
            ("postgres.public.cities", 1),
        ] {
            assert_eq!(
                cassie
                    .midge
                    .scan_documents(collection)
                    .expect("scan collection")
                    .len(),
                expected_rows
            );
        }
        stop_router_test_harness(harness).await;
    });
}

#[test]
fn should_reject_encoded_collection_separators_with_traversal() {
    // Arrange
    let invalid = [
        "cities%2Fevents",
        "cities%5Cevents",
        "%2E%2E",
        "%2E",
        "%",
        "%GG",
        "%FF",
    ];

    // Act
    let errors = invalid.map(collection_routes::decode_collection_segment);

    // Assert
    for (requested, result) in invalid.into_iter().zip(errors) {
        assert!(
            matches!(result, Err(crate::app::CassieError::Parse(_))),
            "{requested} was accepted"
        );
    }
}

#[test]
fn should_reject_a_rest_body_that_stalls_between_frames() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let result = collect_request_body(PendingBody, Duration::from_millis(1)).await;

        // Assert
        assert!(matches!(result, Err(RestBodyReadError::TimedOut)));
    });
}

struct PendingBody;

impl Body for PendingBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        std::task::Poll::Pending
    }

    fn is_end_stream(&self) -> bool {
        false
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::SizeHint::default()
    }
}

#[test]
fn should_emit_hsts_only_for_secure_transport() {
    // Arrange
    let response = json_response(StatusCode::OK, &serde_json::json!({}));
    let secure_response = with_security_headers(response, false, true);
    let response = json_response(StatusCode::OK, &serde_json::json!({}));

    // Act
    let plain_response = with_security_headers(response, false, false);

    // Assert
    assert!(secure_response
        .headers()
        .contains_key("strict-transport-security"));
    assert!(!plain_response
        .headers()
        .contains_key("strict-transport-security"));
}

#[test]
fn should_emit_no_store_for_api_responses() {
    // Arrange
    let response = json_response(StatusCode::OK, &serde_json::json!({}));

    // Act
    let response = with_security_headers(response, true, false);

    // Assert
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
}

#[test]
fn should_reject_encoded_slash_request_paths() {
    // Arrange
    let path = "/api/v1/admin%2Fusers";

    // Act
    let result = canonical_request_path(path);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_encoded_backslash_request_paths() {
    // Arrange
    let path = "/api/v1/admin%5Cusers";

    // Act
    let result = canonical_request_path(path);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_dot_segment_request_paths() {
    // Arrange
    let paths = ["/api/./users", "/api/../users", "/api/%2e%2e/users"];

    // Act
    let results = paths.map(canonical_request_path);

    // Assert
    assert!(results.iter().all(Result::is_err));
}

#[test]
fn should_reject_double_slash_request_paths() {
    // Arrange
    let path = "/api//users";

    // Act
    let result = canonical_request_path(path);

    // Assert
    assert!(result.is_err());
}

fn state_change_request(
    origin: Option<&str>,
    host: Option<&str>,
    content_type: Option<&str>,
    body: &'static [u8],
) -> Request<Full<Bytes>> {
    let mut builder = Request::builder().method(Method::POST).uri("/api/v1/users");
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    if let Some(host) = host {
        builder = builder.header("host", host);
    }
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder
        .body(Full::new(Bytes::from_static(body)))
        .expect("request")
}

#[test]
fn should_reject_cross_origin_state_changes_given_a_missing_host_header() {
    // Arrange
    let request = state_change_request(Some("https://cassie.test"), None, None, b"");

    // Act
    let result = validate_rest_origin(&Method::POST, "/api/v1/users", &request);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_cross_origin_state_changes_given_a_mismatched_origin() {
    // Arrange
    let request = state_change_request(Some("https://other.test"), Some("cassie.test"), None, b"");

    // Act
    let result = validate_rest_origin(&Method::POST, "/api/v1/users", &request);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_allow_same_origin_state_changes_given_matching_origin_and_host() {
    // Arrange
    let request = state_change_request(Some("https://cassie.test"), Some("cassie.test"), None, b"");

    // Act
    let result = validate_rest_origin(&Method::POST, "/api/v1/users", &request);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_require_json_content_type_for_state_changing_api_requests() {
    // Arrange
    let request = state_change_request(None, None, Some("text/plain"), b"payload");

    // Act
    let result = validate_rest_content_type(&Method::POST, "/api/v1/users", &request);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_allow_empty_jsonless_delete_requests() {
    // Arrange
    let request = Request::builder()
        .method(Method::DELETE)
        .uri("/api/v1/users/1")
        .body(Full::new(Bytes::new()))
        .expect("request");

    // Act
    let result = validate_rest_content_type(&Method::DELETE, "/api/v1/users/1", &request);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_create_rest_collection_in_the_request_session_database() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let data_dir = std::env::temp_dir().join(format!(
        "cassie-router-collection-database-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Arc::new(Cassie::new_with_data_dir(&data_dir).expect("cassie"));
    cassie.startup().expect("startup");
    let default_session = cassie.create_session("root", Some("postgres".to_string()));
    cassie
        .execute_sql(&default_session, "CREATE DATABASE analytics", Vec::new())
        .expect("create analytics database");
    let request_context = RestRequestContext {
        session: CassieSession::new("root".to_string(), Some("analytics".to_string())),
        role: None,
        token: None,
        external_user: None,
    };
    let method = Method::POST;
    let segments = ["api", "v1", "collections"];
    let body = Bytes::from_static(br#"{"name":"rest_made","fields":[{"name":"x","type":"int"}]}"#);

    // Act
    let result = runtime.block_on(dispatch_collection_routes(
        &method,
        &segments,
        Arc::clone(&cassie),
        &request_context,
        &body,
        "/api/v1/collections",
        Instant::now(),
    ));

    // Assert
    let response = result
        .expect("collection route")
        .expect("matched collection route");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        cassie
            .midge
            .collection_metadata("analytics.public.rest_made")
            .expect("analytics collection lookup")
            .is_some(),
        "collection must be created in the request session database"
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn should_wait_for_worker_exit_before_acknowledging_a_dropped_query_handler() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
            .expect("cassie"),
    );
    cassie.startup().expect("startup");
    let request_context = RestRequestContext {
        session: cassie.create_session("root", None),
        role: None,
        token: None,
        external_user: None,
    };
    let execute_context = RouteDispatchContext {
        method: &Method::POST,
        segments: &[],
        path: "/api/v1/admin/query-executions",
        query: None,
        started_at: Instant::now(),
        request_context: &request_context,
        secure_transport: false,
        peer_ip: "127.0.0.1".parse().expect("loopback"),
        admitted_error_response: false,
        oidc: None,
    };
    let id = uuid::Uuid::new_v4();
    let id_text = id.to_string();
    let cancel_path = format!("/api/v1/admin/query-operations/{id}");
    let cancel_context = RouteDispatchContext {
        method: &Method::DELETE,
        path: &cancel_path,
        ..execute_context
    };
    let body = RestBytes::from(
        serde_json::json!({"database": cassie.default_database(), "operation_id": id}).to_string(),
    );
    let (worker_started, started) = tokio::sync::oneshot::channel();
    let (release_worker, held) = std::sync::mpsc::channel();
    let (worker_exited, mut exited) = tokio::sync::oneshot::channel();
    let hang_guard = Duration::from_secs(5);

    runtime.block_on(async {
        let mut execute = Box::pin(run_admin_query_operation(
            execute_context,
            cassie,
            &body,
            "rest_admin_query_execute",
            move |_, _, _, cancellation| {
                worker_started.send(()).expect("waiting for worker");
                held.recv_timeout(hang_guard).expect("release worker");
                assert!(cancellation.is_canceled());
                worker_exited.send(()).expect("waiting for worker exit");
                Err(crate::app::CassieError::QueryCanceled)
            },
        ));
        tokio::select! {
            result = &mut execute => panic!("query returned before worker release: {:?}", result.err()),
            result = tokio::time::timeout(hang_guard, started) => {
                result.expect("worker start deadline").expect("worker started");
            }
        }

        // Act
        let mut cancel = Box::pin(cancel_admin_query_operation(cancel_context, &id_text));
        tokio::select! {
            biased;
            _ = &mut cancel => panic!("DELETE returned while the worker was held"),
            () = std::future::ready(()) => {}
        }
        drop(execute);
        let active_while_held = crate::rest::query_operations::is_active_for_test(id);
        tokio::select! {
            biased;
            _ = &mut cancel => panic!("DELETE returned after dropping the handler but before worker exit"),
            () = std::future::ready(()) => {}
        }
        release_worker.send(()).expect("release worker");
        let response = tokio::time::timeout(hang_guard, cancel)
            .await
            .expect("DELETE deadline")
            .expect("cancellation acknowledgement")
            .expect("DELETE response");

        // Assert
        assert!(active_while_held);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(exited.try_recv(), Ok(()));
        assert!(!crate::rest::query_operations::is_active_for_test(id));
        assert!(matches!(
            cancel_admin_query_operation(cancel_context, &id_text).await,
            Err((StatusCode::CONFLICT, _))
        ));
    });
}
