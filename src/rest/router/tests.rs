use super::*;
use http_body_util::Full;

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
fn should_add_hsts_only_for_secure_rest_responses() {
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
