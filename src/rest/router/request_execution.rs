use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Notify, Semaphore};
use tokio::task;
use tokio_rustls::TlsAcceptor;

use crate::app::{Cassie, CassieError};
use crate::rest::static_files::AdminUiStaticFiles;
use crate::runtime::QueryCancellationHandle;

use super::{
    record_rest_error, route, too_many_connections_response, MAX_REST_HEADER_BYTES,
    REST_HEADER_READ_TIMEOUT, REST_REQUEST_TIMEOUT,
};

pub(super) async fn run_server(
    addr: String,
    cassie: Cassie,
    shutdown: Arc<Notify>,
    admin_ui_dir: PathBuf,
) -> Result<(), CassieError> {
    let listen: SocketAddr = addr.parse().map_err(|error| {
        CassieError::Execution(format!("invalid rest address '{addr}': {error}"))
    })?;
    let cassie = Arc::new(cassie);
    let admin_ui = Arc::new(AdminUiStaticFiles::new(admin_ui_dir));
    let tls_config = crate::rest::tls::load_server_config(
        cassie.rest_tls_cert_file.as_deref(),
        cassie.rest_tls_key_file.as_deref(),
    )?;
    let admission = Arc::new(Semaphore::new(
        cassie.runtime.limits().rest_max_connections.max(1),
    ));
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let actual_addr = listener
        .local_addr()
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    cassie.validate_rest_network_listener(actual_addr)?;

    loop {
        tokio::select! {
            biased;
            () = shutdown.notified() => {
                tracing::info!(target: "rest", address = %listen, "shutdown requested");
                break;
            }
            accept = listener.accept() => {
                let (stream, _) = accept.map_err(|error| CassieError::Execution(error.to_string()))?;
                let Ok(permit) = admission.clone().try_acquire_owned() else {
                    let tls_config = tls_config.clone();
                    tokio::spawn(async move {
                        if let Some(config) = tls_config {
                            match TlsAcceptor::from(config).accept(stream).await {
                                Ok(stream) => serve_rejection(stream).await,
                                Err(error) => tracing::warn!(%error, "rest TLS handshake rejected"),
                            }
                        } else {
                            serve_rejection(stream).await;
                        }
                    });
                    continue;
                };
                let cassie = Arc::clone(&cassie);
                let admin_ui = Arc::clone(&admin_ui);
                let tls_config = tls_config.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Some(config) = tls_config {
                        match TlsAcceptor::from(config).accept(stream).await {
                            Ok(stream) => serve_application(stream, cassie, admin_ui, true).await,
                            Err(error) => tracing::warn!(%error, "rest TLS handshake rejected"),
                        }
                    } else {
                        serve_application(stream, cassie, admin_ui, false).await;
                    }
                });
            }
        }
    }

    Ok(())
}

async fn serve_rejection<S>(stream: S)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(|_request: Request<hyper::body::Incoming>| async {
        Ok::<_, Infallible>(too_many_connections_response())
    });
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(REST_HEADER_READ_TIMEOUT)
        .max_buf_size(MAX_REST_HEADER_BYTES)
        .serve_connection(TokioIo::new(stream), service)
        .await;
    if let Err(error) = connection {
        tracing::warn!(%error, "rest admission rejection connection error");
    }
}

async fn serve_application<S>(
    stream: S,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
    secure_transport: bool,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
        let cassie = Arc::clone(&cassie);
        let admin_ui = Arc::clone(&admin_ui);
        async move { route(request, cassie, admin_ui, secure_transport).await }
    });
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(REST_HEADER_READ_TIMEOUT)
        .max_buf_size(MAX_REST_HEADER_BYTES)
        .serve_connection(TokioIo::new(stream), service)
        .await;
    if let Err(error) = connection {
        tracing::warn!(%error, "rest connection error");
    }
}

#[derive(Clone)]
pub(super) struct RestRequestExecution {
    cancellation: QueryCancellationHandle,
    deadline: Instant,
}

impl RestRequestExecution {
    #[must_use]
    pub(super) fn new(timeout: Duration) -> Self {
        Self::with_cancellation(timeout, QueryCancellationHandle::new())
    }

    #[must_use]
    pub(super) fn with_cancellation(
        timeout: Duration,
        cancellation: QueryCancellationHandle,
    ) -> Self {
        let now = Instant::now();
        Self {
            cancellation,
            deadline: now.checked_add(timeout).unwrap_or(now),
        }
    }

    pub(super) async fn run_blocking<T>(
        &self,
        cassie: Arc<Cassie>,
        operation_name: &'static str,
        operation: impl FnOnce(Arc<Cassie>, &QueryCancellationHandle) -> Result<T, CassieError>
            + Send
            + 'static,
    ) -> Result<T, RestBlockingError>
    where
        T: Send + 'static,
    {
        let runtime = cassie.runtime.clone();
        let started_at = Instant::now();
        runtime.record_rest_boundary_started(operation_name);

        let cancellation = self.cancellation.clone();
        let worker_cancellation = cancellation.clone();
        let mut worker = task::spawn_blocking(move || operation(cassie, &worker_cancellation));
        let deadline = tokio::time::Instant::from_std(self.deadline);

        tokio::select! {
            result = &mut worker => finish_worker(
                &runtime,
                operation_name,
                started_at,
                result,
                false,
            ),
            () = tokio::time::sleep_until(deadline) => {
                cancellation.cancel();
                finish_worker(
                    &runtime,
                    operation_name,
                    started_at,
                    worker.await,
                    true,
                )
            }
        }
    }
}

pub(super) async fn run_rest_blocking_route<T>(
    cassie: Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>) -> Result<T, CassieError> + Send + 'static,
) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
{
    run_rest_blocking_route_controlled(
        cassie,
        method,
        path,
        started_at,
        operation_name,
        move |cassie, _cancellation| operation(cassie),
    )
    .await
}

pub(super) async fn run_rest_blocking_route_controlled<T>(
    cassie: Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>, &QueryCancellationHandle) -> Result<T, CassieError>
        + Send
        + 'static,
) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
{
    RestRequestExecution::new(REST_REQUEST_TIMEOUT)
        .run_blocking(Arc::clone(&cassie), operation_name, operation)
        .await
        .map_err(|error| match error {
            RestBlockingError::Engine(error) => {
                record_rest_error(&cassie, method.as_str(), path, started_at, &error)
            }
            RestBlockingError::TimedOut => {
                cassie.runtime.record_rest_request(
                    method.as_str(),
                    path,
                    StatusCode::REQUEST_TIMEOUT.as_u16(),
                    started_at.elapsed(),
                );
                (
                    StatusCode::REQUEST_TIMEOUT,
                    "REST request timed out".to_string(),
                )
            }
        })
}

pub(super) async fn run_rest_blocking_route_with_cancellation<T>(
    cassie: Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
    operation_name: &'static str,
    cancellation: QueryCancellationHandle,
    operation: impl FnOnce(Arc<Cassie>, &QueryCancellationHandle) -> Result<T, CassieError>
        + Send
        + 'static,
) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
{
    RestRequestExecution::with_cancellation(REST_REQUEST_TIMEOUT, cancellation)
        .run_blocking(Arc::clone(&cassie), operation_name, operation)
        .await
        .map_err(|error| match error {
            RestBlockingError::Engine(error) => {
                record_rest_error(&cassie, method.as_str(), path, started_at, &error)
            }
            RestBlockingError::TimedOut => {
                cassie.runtime.record_rest_request(
                    method.as_str(),
                    path,
                    StatusCode::REQUEST_TIMEOUT.as_u16(),
                    started_at.elapsed(),
                );
                (
                    StatusCode::REQUEST_TIMEOUT,
                    "REST request timed out".to_string(),
                )
            }
        })
}

#[derive(Debug)]
pub(super) enum RestBlockingError {
    Engine(CassieError),
    TimedOut,
}

fn finish_worker<T>(
    runtime: &crate::runtime::RuntimeState,
    operation_name: &'static str,
    started_at: Instant,
    result: Result<Result<T, CassieError>, task::JoinError>,
    deadline_fired: bool,
) -> Result<T, RestBlockingError> {
    match result {
        Ok(Ok(value)) => {
            runtime.record_rest_boundary_completed(operation_name, started_at.elapsed());
            Ok(value)
        }
        Ok(Err(CassieError::QueryCancelled | CassieError::DeadlineExceeded)) if deadline_fired => {
            runtime.record_rest_boundary_error(operation_name, started_at.elapsed());
            Err(RestBlockingError::TimedOut)
        }
        Ok(Err(error)) => {
            runtime.record_rest_boundary_error(operation_name, started_at.elapsed());
            Err(RestBlockingError::Engine(error))
        }
        Err(error) => {
            runtime.record_rest_boundary_join_failed(operation_name, started_at.elapsed());
            Err(RestBlockingError::Engine(CassieError::StorageRetryable(
                format!("rest blocking boundary '{operation_name}' failed: {error}"),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn cassie(label: &str) -> Arc<Cassie> {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        Arc::new(
            Cassie::new_with_data_dir(format!(
                "/tmp/cassie-rest-request-execution-{label}-{}",
                Uuid::new_v4()
            ))
            .expect("cassie"),
        )
    }

    #[test]
    fn should_return_408_only_after_worker_acknowledges_cancellation() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let cassie = cassie("acknowledged-timeout");
        let execution = RestRequestExecution::new(Duration::from_millis(1));

        // Act
        let error = runtime.block_on(execution.run_blocking(
            cassie,
            "rest_timeout_test",
            |_cassie, cancellation| {
                while !cancellation.is_cancelled() {
                    std::thread::yield_now();
                }
                Err::<(), _>(CassieError::QueryCancelled)
            },
        ));

        // Assert
        assert!(matches!(error, Err(RestBlockingError::TimedOut)));
    }

    #[test]
    fn should_return_success_when_commit_wins_timeout_race() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let cassie = cassie("commit-wins");
        let execution = RestRequestExecution::new(Duration::from_millis(1));

        // Act
        let result = runtime.block_on(execution.run_blocking(
            cassie,
            "rest_commit_test",
            |_cassie, _cancellation| {
                std::thread::sleep(Duration::from_millis(5));
                Ok("committed")
            },
        ));

        // Assert
        assert_eq!(result.expect("successful commit must win"), "committed");
    }
}
