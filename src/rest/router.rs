use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::Incoming,
    header::{HeaderValue, CONTENT_TYPE},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use tokio::sync::Notify;
use tokio::task;

use crate::app::Cassie;
use crate::catalog::RoleMeta;

pub async fn run(addr: String, cassie: Cassie) -> Result<(), crate::app::CassieError> {
    run_with_shutdown(addr, cassie, Arc::new(Notify::new())).await
}

pub async fn run_with_shutdown(
    addr: String,
    cassie: Cassie,
    shutdown: Arc<Notify>,
) -> Result<(), crate::app::CassieError> {
    let listen: SocketAddr = addr.parse().map_err(|e| {
        crate::app::CassieError::Execution(format!("invalid rest address '{}': {}", addr, e))
    })?;
    let cassie = Arc::new(cassie);
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                tracing::info!(target: "rest", address = %listen, "shutdown requested");
                break;
            }
            accept = listener.accept() => {
                let (stream, _) = accept.map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;
                let cassie = cassie.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let cassie = cassie.clone();
                        async move { route(request, cassie).await }
                    });
                    let io = TokioIo::new(stream);

                    let connection = http1::Builder::new().serve_connection(io, service).await;
                    if let Err(error) = connection {
                        tracing::warn!(%error, "rest connection error");
                    }
                });
            }
        }
    }

    Ok(())
}

type RestBody = Full<Bytes>;
const AUTH_TOKEN_PREFIX: &str = "Bearer ";

async fn route(
    request: Request<hyper::body::Incoming>,
    cassie: Arc<Cassie>,
) -> Result<Response<RestBody>, Infallible> {
    let response = match route_request(request, cassie).await {
        Ok(response) => response,
        Err((status, message)) => json_response(status, &serde_json::json!({ "error": message })),
    };
    Ok(response)
}

type RestBytes = Bytes;

fn json_response<T: serde::Serialize>(status: StatusCode, value: &T) -> Response<RestBody> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::new(Full::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn map_error(error: crate::app::CassieError) -> (StatusCode, String) {
    match error {
        crate::app::CassieError::CollectionNotFound(_) => {
            (StatusCode::NOT_FOUND, error.to_string())
        }
        crate::app::CassieError::NotNullViolation { .. }
        | crate::app::CassieError::UniqueViolation { .. }
        | crate::app::CassieError::CheckViolation { .. }
        | crate::app::CassieError::ForeignKeyViolation { .. } => {
            (StatusCode::CONFLICT, error.to_string())
        }
        crate::app::CassieError::NotFound(_) => (StatusCode::NOT_FOUND, error.to_string()),
        crate::app::CassieError::Parse(_) | crate::app::CassieError::InvalidVector(_) => {
            (StatusCode::BAD_REQUEST, error.to_string())
        }
        crate::app::CassieError::InvalidEmbedding(_) => {
            (StatusCode::BAD_REQUEST, error.to_string())
        }
        crate::app::CassieError::EmbeddingUnavailable(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, error.to_string())
        }
        crate::app::CassieError::Unauthorized => (StatusCode::UNAUTHORIZED, error.to_string()),
        crate::app::CassieError::Unsupported(_) => (StatusCode::NOT_IMPLEMENTED, error.to_string()),
        crate::app::CassieError::StorageBootstrap(_)
        | crate::app::CassieError::StorageMissingFamily(_)
        | crate::app::CassieError::StorageRetryable(_)
        | crate::app::CassieError::Storage(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, error.to_string())
        }
        crate::app::CassieError::Planner(_) | crate::app::CassieError::Execution(_) => {
            (StatusCode::BAD_REQUEST, error.to_string())
        }
    }
}

fn record_rest_error(
    cassie: &Arc<Cassie>,
    method: &str,
    route: &str,
    started_at: Instant,
    error: crate::app::CassieError,
) -> (StatusCode, String) {
    let mapped = map_error(error);
    cassie
        .runtime
        .record_rest_request(method, route, mapped.0.as_u16(), started_at.elapsed());
    mapped
}

pub async fn route_request(
    request: Request<Incoming>,
    cassie: Arc<Cassie>,
) -> Result<Response<RestBody>, (StatusCode, String)> {
    let method = request.method().clone();
    let path = request.uri().path().trim_end_matches('/').to_string();
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path
    };
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let started_at = Instant::now();
    if !is_route_public(&method, segments.as_slice()) && !cassie.auth_password.is_empty() {
        let role = match authenticate_rest_request(cassie.clone(), request.headers()).await {
            Ok(role) => role,
            Err((status, message)) => {
                cassie.runtime.record_rest_request(
                    method.as_str(),
                    &path,
                    status.as_u16(),
                    started_at.elapsed(),
                );
                return Err((status, message));
            }
        };

        if !role.is_admin {
            cassie.runtime.record_rest_request(
                method.as_str(),
                &path,
                StatusCode::FORBIDDEN.as_u16(),
                started_at.elapsed(),
            );
            return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
        }
    }
    let body: RestBytes = request
        .into_body()
        .collect()
        .await
        .map_err(|e| {
            cassie.runtime.record_rest_request(
                method.as_str(),
                &path,
                StatusCode::BAD_REQUEST.as_u16(),
                started_at.elapsed(),
            );
            (StatusCode::BAD_REQUEST, e.to_string())
        })?
        .to_bytes();

    let response = match (method.clone(), segments.as_slice()) {
        (Method::GET, ["health"]) => {
            let value = crate::rest::health::health(&cassie);
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["liveness"]) => {
            let value = crate::rest::health::liveness(&cassie);
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["metrics"]) => {
            let value = crate::rest::health::metrics(&cassie);
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["v1", "collections"]) => {
            let value = run_rest_blocking(cassie.clone(), "rest_route", move |cassie| {
                Ok(crate::rest::collections::list(&cassie))
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections"]) => {
            let body = body.clone();
            let value = run_rest_blocking(cassie.clone(), "rest_route", move |cassie| {
                crate::rest::collections::create(&cassie, body.as_ref())
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections", collection, "documents"]) => {
            let collection = collection.to_string();
            let body = body.clone();
            let value = run_rest_blocking(cassie.clone(), "rest_route", move |cassie| {
                crate::rest::documents::create(&cassie, &collection, body.as_ref())
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections", collection, "indexes"]) => {
            let collection = collection.to_string();
            let body = body.clone();
            let value = run_rest_blocking(cassie.clone(), "rest_route", move |cassie| {
                crate::rest::indexes::create(&cassie, &collection, body.as_ref())
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections", collection, "search"]) => {
            let collection = collection.to_string();
            let body = body.clone();
            let value = run_rest_blocking(cassie.clone(), "rest_embedding_search", move |cassie| {
                crate::rest::search::vector_search(&cassie, &collection, body.as_ref())
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["v1", "collections", collection, "documents", id]) => {
            let collection = collection.to_string();
            let id = id.to_string();
            let value = run_rest_blocking(cassie.clone(), "rest_route", move |cassie| {
                crate::rest::documents::get(&cassie, &collection, &id)
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::DELETE, ["v1", "collections", collection, "documents", id]) => {
            let collection = collection.to_string();
            let id = id.to_string();
            let value = run_rest_blocking(cassie.clone(), "rest_route", move |cassie| {
                crate::rest::documents::delete(&cassie, &collection, &id)
            })
            .await
            .map_err(|error| {
                record_rest_error(&cassie, method.as_str(), &path, started_at, error)
            })?;
            json_response(StatusCode::OK, &value)
        }
        _ => {
            cassie.runtime.record_rest_request(
                method.as_str(),
                &path,
                StatusCode::NOT_FOUND.as_u16(),
                started_at.elapsed(),
            );
            return Err((
                StatusCode::NOT_FOUND,
                format!("unsupported route: {} {}", method, path),
            ));
        }
    };

    cassie.runtime.record_rest_request(
        method.as_str(),
        &path,
        response.status().as_u16(),
        started_at.elapsed(),
    );

    Ok(response)
}

async fn run_rest_blocking<T>(
    cassie: Arc<Cassie>,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>) -> Result<T, crate::app::CassieError> + Send + 'static,
) -> Result<T, crate::app::CassieError>
where
    T: Send + 'static,
{
    let runtime = cassie.runtime.clone();
    let started_at = Instant::now();
    runtime.record_rest_boundary_started(operation_name);

    let result = task::spawn_blocking(move || operation(cassie)).await;

    match result {
        Ok(result) => match result {
            Ok(value) => {
                runtime.record_rest_boundary_completed(operation_name, started_at.elapsed());
                Ok(value)
            }
            Err(error) => {
                runtime.record_rest_boundary_error(operation_name, started_at.elapsed());
                Err(error)
            }
        },
        Err(error) => {
            runtime.record_rest_boundary_join_failed(operation_name, started_at.elapsed());
            Err(crate::app::CassieError::StorageRetryable(format!(
                "rest blocking boundary '{operation_name}' failed: {error}"
            )))
        }
    }
}

fn is_route_public(method: &Method, segments: &[&str]) -> bool {
    matches!(
        (method, segments),
        (&Method::GET, ["health"]) | (&Method::GET, ["liveness"]) | (&Method::GET, ["metrics"])
    )
}

async fn authenticate_rest_request(
    cassie: Arc<Cassie>,
    headers: &hyper::HeaderMap,
) -> Result<RoleMeta, (StatusCode, String)> {
    let Some((user, password)) = parse_rest_credentials(&cassie, headers) else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };

    let role = run_rest_blocking(cassie.clone(), "rest_auth", move |cassie| {
        let session = cassie.authenticate_role(&user, password.as_deref(), None)?;
        let Some(role) = cassie.lookup_role(&session.user).map_err(|error| {
            crate::app::CassieError::StorageRetryable(format!(
                "lookup role '{}': {error}",
                session.user
            ))
        })?
        else {
            return Err(crate::app::CassieError::StorageRetryable(format!(
                "role '{}' disappeared during authentication",
                session.user
            )));
        };

        Ok(role)
    })
    .await
    .map_err(|error| match error {
        crate::app::CassieError::Unauthorized => {
            (StatusCode::UNAUTHORIZED, "unauthorized".to_string())
        }
        other => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("authentication unavailable: {other}"),
        ),
    })?;

    Ok(role)
}

fn parse_rest_credentials(
    cassie: &Arc<Cassie>,
    headers: &hyper::HeaderMap,
) -> Option<(String, Option<String>)> {
    let raw = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())?;
    let token = raw.strip_prefix(AUTH_TOKEN_PREFIX)?.trim();
    if token.is_empty() {
        return None;
    }

    if let Some((user, password)) = token.split_once(':') {
        Some((user.trim().to_string(), Some(password.trim().to_string())))
    } else {
        Some((cassie.auth_user.clone(), Some(token.to_string())))
    }
}
