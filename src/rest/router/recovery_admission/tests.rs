use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::{Method, StatusCode};
use hyper_util::rt::TokioIo;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;

use super::super::request_execution::{RestBlockingError, RestRequestExecution};
use super::super::{AdminUiStaticFiles, RestRequestContext, RouteDispatchContext};

fn instance() -> Arc<Cassie> {
    Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig {
            password: "rest-recovery-fixture-password".to_owned(),
            ..CassieRuntimeConfig::default()
        })
        .expect("memory Cassie"),
    )
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .expect("current-thread runtime")
}

async fn request(
    cassie: Arc<Cassie>,
    method: Method,
    path: &str,
) -> (StatusCode, serde_json::Value) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let service = hyper::service::service_fn(move |request| {
            super::super::route(
                request,
                Arc::clone(&cassie),
                Arc::new(AdminUiStaticFiles::new(std::path::PathBuf::from(
                    "unused-recovery-ui",
                ))),
                false,
                IpAddr::V4(Ipv4Addr::LOCALHOST),
            )
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(socket), service)
            .await;
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client");
    let is_head = method == Method::HEAD;
    let response = client
        .request(method, format!("http://{address}{path}"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .expect("HTTP request");
    let status = response.status();
    let body = if is_head {
        assert!(response.bytes().await.expect("HEAD response").is_empty());
        serde_json::Value::Null
    } else {
        response.json().await.expect("JSON response")
    };
    server.abort();
    let _ = server.await;
    (status, body)
}

#[test]
fn should_keep_only_explicit_health_probes_available_during_storage_recovery() {
    // Arrange
    let cassie = instance();
    cassie
        .catalog
        .register_collection("private_collection_name", vec![]);
    cassie.midge.latch_storage_recovery_required_for_test();
    let cases = [
        ("/health", StatusCode::OK, false),
        ("/health/", StatusCode::OK, false),
        ("/healthz", StatusCode::SERVICE_UNAVAILABLE, false),
        ("/readyz", StatusCode::SERVICE_UNAVAILABLE, false),
        ("/startupz", StatusCode::SERVICE_UNAVAILABLE, false),
        ("/liveness", StatusCode::OK, true),
        ("/livez", StatusCode::OK, true),
        ("/targetz", StatusCode::OK, true),
    ];
    for (path, expected_status, ready) in cases {
        // Act
        let (status, body) = runtime().block_on(request(Arc::clone(&cassie), Method::GET, path));
        // Assert
        assert_eq!(status, expected_status, "{path}");
        assert_eq!(body["ready"], ready, "{path}");
        assert!(!body.to_string().contains("private_collection_name"));
        if !ready {
            assert_eq!(body["status"], "recovery_required");
            assert_eq!(body["collections"], 1);
        }
    }
}

#[test]
fn should_reject_ordinary_rest_work_before_authentication_when_recovery_is_required() {
    // Arrange
    let cassie = instance();
    assert!(cassie.authentication_enabled());
    cassie.midge.latch_storage_recovery_required_for_test();
    let cases = [
        (Method::GET, "/metrics"),
        (Method::GET, "/"),
        (Method::POST, "/api/v1/auth/login"),
        (Method::GET, "/api/v1/auth/session"),
        (Method::POST, "/api/v1/auth/logout"),
        (Method::POST, "/api/v1/query"),
        (Method::GET, "/api/v1/databases"),
        (Method::POST, "/health"),
        (Method::HEAD, "/healthz"),
        (Method::GET, "/Health"),
        (Method::GET, "/livez/nested"),
    ];
    for (method, path) in cases {
        // Act
        let is_head = method == Method::HEAD;
        let (status, body) = runtime().block_on(request(Arc::clone(&cassie), method, path));
        // Assert
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        if is_head {
            continue;
        }
        assert!(body["error"]
            .as_str()
            .is_some_and(|message| message.contains("not submitted")));
    }
}

#[test]
fn should_recheck_storage_recovery_before_dispatch_after_a_request_body_wait() {
    // Arrange
    let cassie = instance();
    let method = Method::GET;
    let path = "/";
    assert!(super::guard(&cassie, &method, path, Instant::now())
        .expect("initial admission")
        .is_none());
    cassie.midge.latch_storage_recovery_required_for_test();
    let request_context = RestRequestContext {
        session: cassie.create_session("root", None),
        role: None,
        token: None,
    };
    let context = RouteDispatchContext {
        method: &method,
        segments: &[],
        path,
        query: None,
        started_at: Instant::now(),
        request_context: &request_context,
        secure_transport: false,
        peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
    };

    // Act
    let result = runtime().block_on(super::super::route_dispatch(
        context,
        cassie,
        Arc::new(AdminUiStaticFiles::new(std::path::PathBuf::from(
            "unused-recovery-ui",
        ))),
        bytes::Bytes::from_static(b"{}"),
    ));

    // Assert
    let Err((status, message)) = result else {
        panic!("dispatch must recheck before static file access");
    };
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(message.contains("not submitted"));
}

#[test]
fn should_recheck_storage_recovery_when_a_queued_rest_worker_starts() {
    // Arrange
    let cassie = instance();
    let called = Arc::new(AtomicBool::new(false));
    let execution = RestRequestExecution::new(Duration::from_secs(5));

    // Act
    let result = runtime().block_on(async {
        let (started, reached) = tokio::sync::oneshot::channel();
        let (release, waiting) = std::sync::mpsc::sync_channel(1);
        let blocker = tokio::task::spawn_blocking(move || {
            started.send(()).expect("blocker reached");
            waiting
                .recv_timeout(Duration::from_secs(5))
                .expect("release blocker");
        });
        reached.await.expect("blocking pool occupied");
        let marker = Arc::clone(&called);
        let poisoned = Arc::clone(&cassie);
        let result = execution
            .run_blocking_observed(
                cassie,
                "rest_recovery_queued_probe",
                move |_, _| {
                    marker.store(true, Ordering::Release);
                    Ok(())
                },
                move || {
                    poisoned.midge.latch_storage_recovery_required_for_test();
                    release.send(()).expect("release queued worker");
                },
            )
            .await;
        blocker.await.expect("blocker exits");
        result
    });

    // Assert
    assert!(!called.load(Ordering::Acquire));
    assert!(matches!(
        result,
        Err(RestBlockingError::Engine(crate::app::CassieError::Storage(
            _
        )))
    ));
}

#[test]
fn should_preserve_registered_collection_health_counts_without_native_reads() {
    // Arrange
    let cassie = instance();
    cassie.startup().expect("start health fixture");
    cassie.catalog.register_collection("cached_only", vec![]);

    // Act
    let healthy = cassie.health();
    cassie.midge.latch_storage_recovery_required_for_test();
    let recovering = cassie.health();

    // Assert
    assert_eq!(healthy["ready"], true);
    assert_eq!(healthy["collections"], 1);
    assert_eq!(recovering["collections"], 1);
    assert_eq!(recovering["ready"], false);
    assert_eq!(recovering["status"], "recovery_required");
}
