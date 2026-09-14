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
