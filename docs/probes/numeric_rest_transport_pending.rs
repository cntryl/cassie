//! Unregistered, unexecuted probe for pending Hyper socket-delivery ownership.
//! This deliberately expresses an invariant not yet implemented; register it
//! only with the connection-level transport ownership follow-up.
use super::*;
use crate::config::CassieRuntimeLimits;
use http_body_util::BodyExt;
use std::time::Instant;

struct PendingHttpWrite {
    request: Option<&'static [u8]>,
    write_attempted: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl tokio::io::AsyncRead for PendingHttpWrite {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
        output: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if let Some(request) = self.request.take() {
            output.put_slice(request);
            std::task::Poll::Ready(Ok(()))
        } else {
            std::task::Poll::Pending
        }
    }
}

impl tokio::io::AsyncWrite for PendingHttpWrite {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
        _bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.write_attempted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        std::task::Poll::Pending
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[test]
fn should_retain_numeric_error_body_while_hyper_socket_write_is_pending() {
    // Arrange
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::Poll;

    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1_048_576,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let source = crate::app::OwnedNumericLiteralError::syntax(&"bad".repeat(1024), &controls)
        .expect("source");
    let response = numeric_error_response(&source).expect("response");
    drop(source);
    let body_emitted = std::sync::Arc::new(AtomicBool::new(false));
    let emitted = body_emitted.clone();
    let response = response.map(|body| {
        body.map_frame(move |frame| {
            emitted.store(true, Ordering::SeqCst);
            frame
        })
        .boxed_unsync()
    });
    let attempted = std::sync::Arc::new(AtomicBool::new(false));
    let io = PendingHttpWrite {
        request: Some(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n"),
        write_attempted: attempted.clone(),
    };
    let transport =
        crate::transport::TimedWriteTransport::new(io, std::time::Duration::from_secs(30));
    let mut response = Some(response);
    let service = hyper::service::service_fn(move |_| {
        let response = response.take().expect("one request");
        async move { Ok::<_, std::convert::Infallible>(response) }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut connection = Box::pin(
        hyper::server::conn::http1::Builder::new()
            .serve_connection(hyper_util::rt::TokioIo::new(transport), service),
    );

    // Act
    runtime.block_on(std::future::poll_fn(|context| {
        assert!(connection.as_mut().poll(context).is_pending());
        Poll::Ready(())
    }));

    // Assert
    assert!(attempted.load(Ordering::SeqCst));
    assert!(body_emitted.load(Ordering::SeqCst));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(connection);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
