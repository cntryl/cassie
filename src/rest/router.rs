use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::Incoming,
    header::{HeaderValue, ALLOW, CONNECTION, CONTENT_TYPE},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use tokio::sync::{Notify, Semaphore};
use tokio::task;

use crate::app::Cassie;
use crate::catalog::RoleMeta;
use crate::rest::static_files::AdminUiStaticFiles;

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub async fn run(addr: String, cassie: Cassie) -> Result<(), crate::app::CassieError> {
    run_with_shutdown(addr, cassie, Arc::new(Notify::new())).await
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub async fn run_with_shutdown(
    addr: String,
    cassie: Cassie,
    shutdown: Arc<Notify>,
) -> Result<(), crate::app::CassieError> {
    run_with_shutdown_and_admin_ui_dir(addr, cassie, shutdown, default_admin_ui_dir()).await
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub async fn run_with_shutdown_and_admin_ui_dir(
    addr: String,
    cassie: Cassie,
    shutdown: Arc<Notify>,
    admin_ui_dir: PathBuf,
) -> Result<(), crate::app::CassieError> {
    let listen: SocketAddr = addr.parse().map_err(|e| {
        crate::app::CassieError::Execution(format!("invalid rest address '{addr}': {e}"))
    })?;
    let cassie = Arc::new(cassie);
    let admin_ui = Arc::new(AdminUiStaticFiles::new(admin_ui_dir));
    let admission = Arc::new(Semaphore::new(
        cassie.runtime.limits().rest_max_connections.max(1),
    ));
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;

    loop {
        tokio::select! {
            biased;
            () = shutdown.notified() => {
                tracing::info!(target: "rest", address = %listen, "shutdown requested");
                break;
            }
            accept = listener.accept() => {
                let (stream, _) = accept.map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;
                let Ok(permit) = admission.clone().try_acquire_owned() else {
                    tokio::spawn(async move {
                        let service = service_fn(|_request: Request<hyper::body::Incoming>| async {
                            Ok::<_, Infallible>(too_many_connections_response())
                        });
                        let io = TokioIo::new(stream);
                        let connection = http1::Builder::new().serve_connection(io, service).await;
                        if let Err(error) = connection {
                            tracing::warn!(%error, "rest admission rejection connection error");
                        }
                    });
                    continue;
                };
                let cassie = cassie.clone();
                let admin_ui = admin_ui.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let cassie = cassie.clone();
                        let admin_ui = admin_ui.clone();
                        async move { route(request, cassie, admin_ui).await }
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
#[derive(Clone, Copy)]
struct RouteDispatchContext<'a> {
    method: &'a Method,
    segments: &'a [&'a str],
    path: &'a str,
    started_at: Instant,
    authenticated_role: Option<&'a RoleMeta>,
}

async fn route(
    request: Request<hyper::body::Incoming>,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
) -> Result<Response<RestBody>, Infallible> {
    let response = match route_request_with_admin_ui(request, cassie, admin_ui).await {
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

fn too_many_connections_response() -> Response<RestBody> {
    let mut response = json_response(
        StatusCode::SERVICE_UNAVAILABLE,
        &serde_json::json!({"error": "too many connections"}),
    );
    response
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));
    response
}

fn method_not_allowed_response(allow: &'static str) -> Response<RestBody> {
    let mut response = json_response(
        StatusCode::METHOD_NOT_ALLOWED,
        &serde_json::json!({"error": "method not allowed"}),
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}

fn map_error(error: &crate::app::CassieError) -> (StatusCode, String) {
    let descriptor = error.descriptor();
    (
        StatusCode::from_u16(descriptor.http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        descriptor.message,
    )
}

fn record_rest_error(
    cassie: &Arc<Cassie>,
    method: &str,
    route: &str,
    started_at: Instant,
    error: &crate::app::CassieError,
) -> (StatusCode, String) {
    let mapped = map_error(error);
    cassie
        .runtime
        .record_rest_request(method, route, mapped.0.as_u16(), started_at.elapsed());
    mapped
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub async fn route_request(
    request: Request<Incoming>,
    cassie: Arc<Cassie>,
) -> Result<Response<RestBody>, (StatusCode, String)> {
    route_request_with_admin_ui(
        request,
        cassie,
        Arc::new(AdminUiStaticFiles::new(default_admin_ui_dir())),
    )
    .await
}

async fn route_request_with_admin_ui(
    request: Request<Incoming>,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
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
    let mut authenticated_role = None;
    if !is_route_public(&method, segments.as_slice()) && cassie.authentication_enabled() {
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
        authenticated_role = Some(role);
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

    let response = route_dispatch(
        RouteDispatchContext {
            method: &method,
            segments: segments.as_slice(),
            path: &path,
            started_at,
            authenticated_role: authenticated_role.as_ref(),
        },
        cassie.clone(),
        admin_ui,
        body,
    )
    .await?;

    cassie.runtime.record_rest_request(
        method.as_str(),
        &path,
        response.status().as_u16(),
        started_at.elapsed(),
    );

    Ok(response)
}

async fn route_dispatch(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
    body: RestBytes,
) -> Result<Response<RestBody>, (StatusCode, String)> {
    if let Some(response) = dispatch_health_routes(context.method, context.segments, &cassie) {
        return Ok(response);
    }

    if let Some(response) = admin_ui.dispatch(context.method, context.segments).await {
        return Ok(response);
    }

    if let Some(response) = dispatch_collection_routes(
        context.method,
        context.segments,
        cassie.clone(),
        &body,
        context.path,
        context.started_at,
    )
    .await?
    {
        return Ok(response);
    }

    if let Some(response) = dispatch_admin_routes(context, cassie.clone(), &body).await? {
        return Ok(response);
    }

    unsupported_route(&cassie, context.method, context.path, context.started_at)
}

fn dispatch_health_routes(
    method: &Method,
    segments: &[&str],
    cassie: &Arc<Cassie>,
) -> Option<Response<RestBody>> {
    match (method.as_str(), segments) {
        ("GET", ["health"]) => Some(json_response(
            StatusCode::OK,
            &crate::rest::health::health(cassie),
        )),
        ("GET", ["liveness"]) => Some(json_response(
            StatusCode::OK,
            &crate::rest::health::liveness(cassie),
        )),
        ("GET", ["targetz"]) => Some(json_response(
            StatusCode::OK,
            &crate::rest::health::targetz(cassie),
        )),
        ("GET", ["metrics"]) => Some(json_response(
            StatusCode::OK,
            &crate::rest::health::metrics(cassie),
        )),
        _ => None,
    }
}

async fn dispatch_collection_routes(
    method: &Method,
    segments: &[&str],
    cassie: Arc<Cassie>,
    body: &RestBytes,
    path: &str,
    started_at: Instant,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    match (method.as_str(), segments) {
        ("GET", ["api", "v1", "collections"]) => run_rest_blocking_route(
            cassie,
            method,
            path,
            started_at,
            "rest_route",
            move |cassie| Ok(crate::rest::collections::list(&cassie)),
        )
        .await
        .map(|value| Some(json_response(StatusCode::OK, &value))),
        ("POST", ["api", "v1", "collections"]) => {
            let body = body.clone();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::collections::create(&cassie, body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("POST", ["api", "v1", "collections", collection, "documents"]) => {
            let body = body.clone();
            let collection = (*collection).to_string();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::documents::create(&cassie, &collection, body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("POST", ["api", "v1", "collections", collection, "indexes"]) => {
            let body = body.clone();
            let collection = (*collection).to_string();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::indexes::create(&cassie, &collection, body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("POST", ["api", "v1", "collections", collection, "search"]) => {
            let body = body.clone();
            let collection = (*collection).to_string();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_embedding_search",
                move |cassie| {
                    crate::rest::search::vector_search(&cassie, &collection, body.as_ref())
                },
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("GET", ["api", "v1", "collections", collection, "documents", id]) => {
            let collection = (*collection).to_string();
            let id = (*id).to_string();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::documents::get(&cassie, &collection, &id),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("DELETE", ["api", "v1", "collections", collection, "documents", id]) => {
            let collection = (*collection).to_string();
            let id = (*id).to_string();
            run_rest_blocking_route(
                cassie,
                method,
                path,
                started_at,
                "rest_route",
                move |cassie| crate::rest::documents::delete(&cassie, &collection, &id),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        _ => Ok(None),
    }
}

async fn dispatch_admin_routes(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    body: &RestBytes,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    if let Some(response) = dispatch_admin_query_routes(context, cassie.clone(), body).await? {
        return Ok(Some(response));
    }
    if let Some(allow) = admin_query_route_allow(context.segments) {
        return Ok(Some(method_not_allowed_response(allow)));
    }

    match (context.method.as_str(), context.segments) {
        (
            "POST",
            ["api", "v1", "admin", "projections", projection, "verification-manifest" | "verification-manifests"],
        ) => {
            let body = body.clone();
            let projection = (*projection).to_string();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_route",
                move |cassie| {
                    crate::rest::consistency::export_manifest(&cassie, &projection, body.as_ref())
                },
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        (
            "POST",
            ["api", "v1", "admin", "projection-consistency-checks" | "projection-consistency-reports"],
        ) => {
            let body = body.clone();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_route",
                move |cassie| crate::rest::consistency::compare_manifests(&cassie, body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        ("GET", ["api", "v1", "admin", "projection-consistency-reports"]) => {
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_route",
                move |cassie| Ok(crate::rest::consistency::reports(&cassie)),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        _ => Ok(None),
    }
}

async fn dispatch_admin_query_routes(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    body: &RestBytes,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    match (context.method.as_str(), context.segments) {
        ("GET", ["api", "v1", "admin", "query", "schema"] | ["api", "v1", "admin", "catalog"]) => {
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_schema",
                move |cassie| Ok(crate::rest::query::schema(&cassie)),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        (
            "POST",
            ["api", "v1", "admin", "query", "execute"] | ["api", "v1", "admin", "query-executions"],
        ) => {
            let body = body.clone();
            let user = rest_session_user(&cassie, context.authenticated_role);
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_execute",
                move |cassie| crate::rest::query::execute(&cassie, user.as_str(), body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        (
            "POST",
            ["api", "v1", "admin", "query", "validate"]
            | ["api", "v1", "admin", "query-validations"],
        ) => {
            let body = body.clone();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_validate",
                move |cassie| crate::rest::query::validate(&cassie, body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        (
            "POST",
            ["api", "v1", "admin", "query", "explain"]
            | ["api", "v1", "admin", "query-explanations"],
        ) => {
            let body = body.clone();
            let user = rest_session_user(&cassie, context.authenticated_role);
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_explain",
                move |cassie| crate::rest::query::explain(&cassie, user.as_str(), body.as_ref()),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        _ => Ok(None),
    }
}

fn admin_query_route_allow(segments: &[&str]) -> Option<&'static str> {
    match segments {
        ["api", "v1", "admin", "query", "schema"] | ["api", "v1", "admin", "catalog"] => {
            Some("GET")
        }
        ["api", "v1", "admin", "query", "execute" | "validate" | "explain"] => Some("POST"),
        ["api", "v1", "admin", "query-executions" | "query-validations" | "query-explanations"] => {
            Some("POST")
        }
        _ => None,
    }
}

fn rest_session_user(cassie: &Cassie, authenticated_role: Option<&RoleMeta>) -> String {
    authenticated_role.map_or_else(|| cassie.auth_user.clone(), |role| role.name.clone())
}

fn unsupported_route(
    cassie: &Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
) -> Result<Response<RestBody>, (StatusCode, String)> {
    cassie.runtime.record_rest_request(
        method.as_str(),
        path,
        StatusCode::NOT_FOUND.as_u16(),
        started_at.elapsed(),
    );
    Err((
        StatusCode::NOT_FOUND,
        format!("unsupported route: {method} {path}"),
    ))
}

async fn run_rest_blocking_route<T>(
    cassie: Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>) -> Result<T, crate::app::CassieError> + Send + 'static,
) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
{
    run_rest_blocking(cassie.clone(), operation_name, operation)
        .await
        .map_err(|error| record_rest_error(&cassie, method.as_str(), path, started_at, &error))
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
    *method == Method::GET && segments.first() != Some(&"api")
}

fn default_admin_ui_dir() -> PathBuf {
    std::env::var("CASSIE_ADMIN_UI_DIR").map_or_else(|_| PathBuf::from("./ui/dist"), PathBuf::from)
}

async fn authenticate_rest_request(
    cassie: Arc<Cassie>,
    headers: &hyper::HeaderMap,
) -> Result<RoleMeta, (StatusCode, String)> {
    let Some((user, password)) = parse_rest_credentials(&cassie, headers) else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };

    let role = run_rest_blocking(cassie.clone(), "rest_auth", move |cassie| {
        cassie
            .authenticate_principal(&user, password.as_deref(), None)
            .map(|principal| principal.role)
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn data_dir(label: &str) -> String {
        format!("/tmp/cassie-rest-router-{label}-{}", Uuid::new_v4())
    }

    #[test]
    fn should_map_retryable_rest_boundary_failures_to_service_unavailable() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Arc::new(Cassie::new_with_data_dir(data_dir("retryable")).expect("cassie"));

            // Act
            let error = run_rest_blocking(
                cassie,
                "rest_retryable",
                |_| -> Result<(), crate::app::CassieError> {
                    panic!("synthetic rest join failure");
                },
            )
            .await
            .expect_err("panic should map to retryable storage");
            let mapped = map_error(&error);

            // Assert
            assert!(matches!(
                error,
                crate::app::CassieError::StorageRetryable(_)
            ));
            assert_eq!(mapped.0, StatusCode::SERVICE_UNAVAILABLE);
            assert!(mapped
                .1
                .contains("rest blocking boundary 'rest_retryable' failed"));
        });
    }
}
