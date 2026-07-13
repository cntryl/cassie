use std::convert::Infallible;
use std::error::Error;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::{
    body::{Body, Incoming},
    header::{HeaderMap, HeaderValue, ALLOW, CACHE_CONTROL, CONNECTION, CONTENT_TYPE, SET_COOKIE},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Notify, Semaphore};
use tokio::task;
use tokio_rustls::TlsAcceptor;

use crate::app::Cassie;
use crate::app::CassieSession;
use crate::catalog::RoleMeta;
use crate::rest::static_files::AdminUiStaticFiles;

const MAX_REST_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_REST_HEADER_BYTES: usize = 32 * 1024;
const REST_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const REST_BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const REST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    let tls_config = crate::rest::tls::load_server_config(
        cassie.rest_tls_cert_file.as_deref(),
        cassie.rest_tls_key_file.as_deref(),
    )?;
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
                let cassie = cassie.clone();
                let admin_ui = admin_ui.clone();
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
    let io = TokioIo::new(stream);
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(REST_HEADER_READ_TIMEOUT)
        .max_buf_size(MAX_REST_HEADER_BYTES)
        .serve_connection(io, service)
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
        let cassie = cassie.clone();
        let admin_ui = admin_ui.clone();
        async move { route_with_timeout(request, cassie, admin_ui, secure_transport).await }
    });
    let io = TokioIo::new(stream);
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(REST_HEADER_READ_TIMEOUT)
        .max_buf_size(MAX_REST_HEADER_BYTES)
        .serve_connection(io, service)
        .await;
    if let Err(error) = connection {
        tracing::warn!(%error, "rest connection error");
    }
}

async fn route_with_timeout(
    request: Request<hyper::body::Incoming>,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
    secure_transport: bool,
) -> Result<Response<RestBody>, Infallible> {
    apply_request_timeout(
        route(request, cassie, admin_ui, secure_transport),
        REST_REQUEST_TIMEOUT,
    )
    .await
}

async fn apply_request_timeout<F>(
    future: F,
    timeout: Duration,
) -> Result<Response<RestBody>, Infallible>
where
    F: Future<Output = Result<Response<RestBody>, Infallible>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(response) => response,
        Err(_) => Ok(json_response(
            StatusCode::REQUEST_TIMEOUT,
            &serde_json::json!({ "error": "REST request timed out" }),
        )),
    }
}

type RestBody = Full<Bytes>;
#[derive(Clone)]
struct RestRequestContext {
    session: CassieSession,
    role: Option<RoleMeta>,
    token: Option<String>,
}

#[derive(Clone, Copy)]
struct RouteDispatchContext<'a> {
    method: &'a Method,
    segments: &'a [&'a str],
    path: &'a str,
    started_at: Instant,
    request_context: &'a RestRequestContext,
    secure_transport: bool,
}

async fn route(
    request: Request<hyper::body::Incoming>,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
    secure_transport: bool,
) -> Result<Response<RestBody>, Infallible> {
    let is_api = request.uri().path().starts_with("/api/");
    let response = match route_request_with_admin_ui(request, cassie, admin_ui, secure_transport)
        .await
    {
        Ok(response) => response,
        Err((status, message)) => json_response(status, &serde_json::json!({ "error": message })),
    };
    Ok(with_security_headers(response, is_api, secure_transport))
}

fn with_security_headers(
    mut response: Response<RestBody>,
    is_api: bool,
    secure_transport: bool,
) -> Response<RestBody> {
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'; object-src 'none'; frame-ancestors 'none'"),
    );
    if is_api {
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if secure_transport {
        response.headers_mut().insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

type RestBytes = Bytes;

#[derive(Debug)]
enum RestBodyReadError {
    TimedOut,
    TooLarge,
    Invalid(String),
}

async fn read_request_body(
    request: Request<Incoming>,
    cassie: &Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
) -> Result<RestBytes, (StatusCode, String)> {
    if request
        .body()
        .size_hint()
        .upper()
        .is_some_and(|length| length > MAX_REST_BODY_BYTES as u64)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("request body exceeds {MAX_REST_BODY_BYTES} bytes"),
        ));
    }

    match collect_request_body(request.into_body(), REST_BODY_IDLE_TIMEOUT).await {
        Ok(body) => Ok(body),
        Err(RestBodyReadError::TimedOut) => Err((
            StatusCode::REQUEST_TIMEOUT,
            "request body idle timeout".to_string(),
        )),
        Err(RestBodyReadError::TooLarge) => Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("request body exceeds {MAX_REST_BODY_BYTES} bytes"),
        )),
        Err(RestBodyReadError::Invalid(message)) => {
            cassie.runtime.record_rest_request(
                method.as_str(),
                path,
                StatusCode::BAD_REQUEST.as_u16(),
                started_at.elapsed(),
            );
            Err((StatusCode::BAD_REQUEST, message))
        }
    }
}

async fn collect_request_body<B>(
    body: B,
    idle_timeout: Duration,
) -> Result<RestBytes, RestBodyReadError>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
{
    let mut body = Limited::new(body, MAX_REST_BODY_BYTES);
    let mut bytes = BytesMut::new();
    loop {
        let frame = tokio::time::timeout(idle_timeout, body.frame())
            .await
            .map_err(|_| RestBodyReadError::TimedOut)?;
        let Some(frame) = frame else {
            return Ok(bytes.freeze());
        };
        let frame = frame.map_err(|error| {
            if error.downcast_ref::<LengthLimitError>().is_some() {
                RestBodyReadError::TooLarge
            } else {
                RestBodyReadError::Invalid(error.to_string())
            }
        })?;
        if let Ok(data) = frame.into_data() {
            bytes.extend_from_slice(&data);
        }
    }
}

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
        false,
    )
    .await
}

async fn route_request_with_admin_ui(
    request: Request<Incoming>,
    cassie: Arc<Cassie>,
    admin_ui: Arc<AdminUiStaticFiles>,
    secure_transport: bool,
) -> Result<Response<RestBody>, (StatusCode, String)> {
    let method = request.method().clone();
    let path = normalized_request_path(request.uri().path());
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let started_at = Instant::now();
    validate_rest_origin(&method, &path, &request)?;
    validate_rest_content_type(&method, &path, &request)?;
    let mut role = None;
    let mut token = None;
    let mut session = None;
    if !is_route_public(&method, segments.as_slice())
        && !is_authentication_exempt(&method, segments.as_slice())
        && cassie.authentication_enabled()
    {
        let (authenticated_session, authenticated_role, authenticated_token) =
            match authenticate_rest_request(cassie.clone(), request.headers()).await {
                Ok(principal) => (
                    principal.session,
                    principal.role,
                    cookie_token(request.headers()),
                ),
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

        if !authenticated_role.is_admin && !is_read_only_sql_route(&method, segments.as_slice()) {
            cassie.runtime.record_rest_request(
                method.as_str(),
                &path,
                StatusCode::FORBIDDEN.as_u16(),
                started_at.elapsed(),
            );
            return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
        }
        session = Some(authenticated_session);
        role = Some(authenticated_role);
        token = authenticated_token;
    }
    let session = session.unwrap_or_else(|| {
        cassie.create_session(&cassie.auth_user, Some(cassie.default_database.clone()))
    });
    let request_context = RestRequestContext {
        session,
        role,
        token,
    };
    let body = read_request_body(request, &cassie, &method, &path, started_at).await?;

    let response = route_dispatch(
        RouteDispatchContext {
            method: &method,
            segments: segments.as_slice(),
            path: &path,
            started_at,
            request_context: &request_context,
            secure_transport,
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

fn requires_json_content_type(method: &Method, path: &str) -> bool {
    path.starts_with("/api/") && matches!(method, &Method::POST | &Method::PUT | &Method::PATCH)
}

fn validate_rest_origin(
    method: &Method,
    path: &str,
    request: &Request<Incoming>,
) -> Result<(), (StatusCode, String)> {
    if !path.starts_with("/api/")
        || !matches!(
            method,
            &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE
        )
    {
        return Ok(());
    }
    let Some(origin) = request.headers().get("origin") else {
        return Ok(());
    };
    let Some(host) = request.headers().get("host") else {
        return Err((
            StatusCode::FORBIDDEN,
            "REST state-changing requests require a Host header".to_string(),
        ));
    };
    let origin = origin.to_str().ok();
    let host = host.to_str().ok();
    let same_origin = origin
        .zip(host)
        .and_then(|(origin, host)| origin.split_once("://").map(|(_, value)| (value, host)))
        .is_some_and(|(origin_host, host)| origin_host.trim_end_matches('/') == host);
    if same_origin {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "cross-origin REST state change rejected".to_string(),
        ))
    }
}

fn normalized_request_path(raw_path: &str) -> String {
    let path = raw_path.trim_end_matches('/');
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

fn validate_rest_content_type(
    method: &Method,
    path: &str,
    request: &Request<Incoming>,
) -> Result<(), (StatusCode, String)> {
    if requires_json_content_type(method, path)
        && request_has_body(request)
        && !has_json_content_type(request.headers())
    {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "REST API request content type must be application/json".to_string(),
        ));
    }
    Ok(())
}

fn request_has_body(request: &Request<Incoming>) -> bool {
    request
        .body()
        .size_hint()
        .upper()
        .is_none_or(|length| length > 0)
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
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

    if let Some(response) = dispatch_auth_routes(context, cassie.clone(), &body).await? {
        return Ok(response);
    }

    if let Some(response) = admin_ui.dispatch(context.method, context.segments).await {
        return Ok(response);
    }

    if let Some(response) = dispatch_collection_routes(
        context.method,
        context.segments,
        cassie.clone(),
        context.request_context,
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

async fn dispatch_auth_routes(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    body: &RestBytes,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    match (context.method.as_str(), context.segments) {
        ("POST", ["api", "v1", "auth", "login"]) => {
            let body = body.clone();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_auth_login",
                move |cassie| crate::rest::sessions::login(&cassie, body.as_ref()),
            )
            .await
            .map(|(token, principal)| {
                let mut response = json_response(
                    StatusCode::OK,
                    &serde_json::json!({
                        "user": principal.role.name,
                        "database": principal.session.current_database(),
                        "role": principal.role.name,
                    }),
                );
                set_session_cookie(&mut response, &token, false, context.secure_transport);
                Some(response)
            })
        }
        ("GET", ["api", "v1", "auth", "session"]) => {
            if context.request_context.role.is_none() && cassie.authentication_enabled() {
                return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
            }
            Ok(Some(json_response(
                StatusCode::OK,
                &serde_json::json!({
                    "user": context.request_context.session.user,
                    "database": context.request_context.session.current_database(),
                    "role": context.request_context.role.as_ref().map(|role| role.name.clone()),
                }),
            )))
        }
        ("POST", ["api", "v1", "auth", "logout"]) => {
            if let Some(token) = context.request_context.token.as_deref() {
                let token = token.to_string();
                run_rest_blocking_route(
                    cassie,
                    context.method,
                    context.path,
                    context.started_at,
                    "rest_auth_logout",
                    move |cassie| crate::rest::sessions::revoke(&cassie, &token),
                )
                .await?;
            }
            let mut response =
                json_response(StatusCode::OK, &serde_json::json!({"logged_out": true}));
            set_session_cookie(&mut response, "", true, context.secure_transport);
            Ok(Some(response))
        }
        _ => Ok(None),
    }
}

fn set_session_cookie(response: &mut Response<RestBody>, token: &str, clear: bool, secure: bool) {
    let secure_attribute = if secure { "; Secure" } else { "" };
    let value = if clear {
        format!(
            "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict{secure_attribute}",
            crate::rest::sessions::SESSION_COOKIE,
        )
    } else {
        format!(
            "{}={token}; Path=/; HttpOnly; SameSite=Strict{secure_attribute}",
            crate::rest::sessions::SESSION_COOKIE,
        )
    };
    if let Ok(header) = HeaderValue::from_str(&value) {
        response.headers_mut().insert(SET_COOKIE, header);
    }
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
    request_context: &RestRequestContext,
    body: &RestBytes,
    path: &str,
    started_at: Instant,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    match (method.as_str(), segments) {
        ("GET", ["api", "v1", "collections"]) => {
            dispatch_collection_list(cassie, method, path, started_at, request_context).await
        }
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
            let collection = scoped_collection(&cassie, request_context, collection)?;
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
            let collection = scoped_collection(&cassie, request_context, collection)?;
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
            let collection = scoped_collection(&cassie, request_context, collection)?;
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
            let collection = scoped_collection(&cassie, request_context, collection)?;
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
            let collection = scoped_collection(&cassie, request_context, collection)?;
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

async fn dispatch_collection_list(
    cassie: Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
    request_context: &RestRequestContext,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    let session = request_context.session.clone();
    run_rest_blocking_route(
        cassie,
        method,
        path,
        started_at,
        "rest_route",
        move |cassie| Ok(crate::rest::scope::list_collections(&cassie, &session)),
    )
    .await
    .map(|value| Some(json_response(StatusCode::OK, &value)))
}

fn scoped_collection(
    cassie: &Cassie,
    request_context: &RestRequestContext,
    requested: &str,
) -> Result<String, (StatusCode, String)> {
    crate::rest::scope::resolve_collection(cassie, &request_context.session, requested)
        .map_err(|error| map_error(&error))
}

async fn dispatch_admin_routes(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    body: &RestBytes,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    let _authenticated_role = context.request_context.role.as_ref();
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
            let session = context.request_context.session.clone();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_execute",
                move |cassie| {
                    crate::rest::query::execute_with_session(&cassie, &session, body.as_ref())
                },
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
            let session = context.request_context.session.clone();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_validate",
                move |cassie| {
                    crate::rest::query::validate_with_session(&cassie, &session, body.as_ref())
                },
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
            let session = context.request_context.session.clone();
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_explain",
                move |cassie| {
                    crate::rest::query::explain_with_session(&cassie, &session, body.as_ref())
                },
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

fn is_read_only_sql_route(method: &Method, segments: &[&str]) -> bool {
    matches!(
        (method.as_str(), segments),
        (
            "GET",
            ["api", "v1", "admin", "query", "schema"] | ["api", "v1", "admin", "catalog"]
        ) | (
            "POST",
            [
                "api",
                "v1",
                "admin",
                "query",
                "execute" | "validate" | "explain"
            ] | [
                "api",
                "v1",
                "admin",
                "query-executions" | "query-validations" | "query-explanations"
            ]
        )
    )
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

fn is_authentication_exempt(method: &Method, segments: &[&str]) -> bool {
    matches!(
        (method.as_str(), segments),
        ("POST", ["api", "v1", "auth", "login"])
    )
}

fn default_admin_ui_dir() -> PathBuf {
    std::env::var("CASSIE_ADMIN_UI_DIR").map_or_else(|_| PathBuf::from("./ui/dist"), PathBuf::from)
}

async fn authenticate_rest_request(
    cassie: Arc<Cassie>,
    headers: &HeaderMap,
) -> Result<crate::app::AuthenticatedPrincipal, (StatusCode, String)> {
    let Some(token) = cookie_token(headers) else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };

    run_rest_blocking(cassie, "rest_auth", move |cassie| {
        crate::rest::sessions::authenticate(&cassie, &token)
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
    })
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("cookie")?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == crate::rest::sessions::SESSION_COOKIE && !value.is_empty())
            .then(|| value.to_string())
    })
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

    #[test]
    fn should_map_expired_rest_request_deadline_to_request_timeout() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let response = apply_request_timeout(
                async {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    Ok(json_response(StatusCode::OK, &serde_json::json!({})))
                },
                Duration::from_millis(1),
            )
            .await
            .expect("timeout response");

            // Assert
            assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        });
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
}
