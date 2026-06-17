use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    header::{HeaderValue, CONTENT_TYPE},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;

use crate::app::Cassie;

pub async fn run(addr: String, cassie: Cassie) -> Result<(), crate::app::CassieError> {
    let listen: SocketAddr = addr.parse().map_err(|e| {
        crate::app::CassieError::Execution(format!("invalid rest address '{}': {}", addr, e))
    })?;
    let cassie = Arc::new(cassie);
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;
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

type RestBody = Full<Bytes>;
type RestRequestBody = hyper::body::Incoming;

async fn route(
    request: Request<RestRequestBody>,
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

async fn route_request(
    request: Request<RestRequestBody>,
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
            let value = crate::rest::health::health(&cassie).await;
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["metrics"]) => {
            let value = crate::rest::health::metrics(&cassie).await;
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["v1", "collections"]) => {
            let value = crate::rest::collections::list(&cassie).await;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections"]) => {
            let value = crate::rest::collections::create(&cassie, body.as_ref())
                .await
                .map_err(|error| {
                    record_rest_error(&cassie, method.as_str(), &path, started_at, error)
                })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections", collection, "documents"]) => {
            let value = crate::rest::documents::create(&cassie, collection, body.as_ref())
                .await
                .map_err(|error| {
                    record_rest_error(&cassie, method.as_str(), &path, started_at, error)
                })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections", collection, "indexes"]) => {
            let value = crate::rest::indexes::create(&cassie, collection, body.as_ref())
                .await
                .map_err(|error| {
                    record_rest_error(&cassie, method.as_str(), &path, started_at, error)
                })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::POST, ["v1", "collections", collection, "search"]) => {
            let value = crate::rest::search::vector_search(&cassie, collection, body.as_ref())
                .await
                .map_err(|error| {
                    record_rest_error(&cassie, method.as_str(), &path, started_at, error)
                })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::GET, ["v1", "collections", collection, "documents", id]) => {
            let value = crate::rest::documents::get(&cassie, collection, id)
                .await
                .map_err(|error| {
                    record_rest_error(&cassie, method.as_str(), &path, started_at, error)
                })?;
            json_response(StatusCode::OK, &value)
        }
        (Method::DELETE, ["v1", "collections", collection, "documents", id]) => {
            let value = crate::rest::documents::delete(&cassie, collection, id)
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
