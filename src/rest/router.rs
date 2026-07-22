use std::convert::Infallible;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::{
    body::{Body, Incoming},
    header::{HeaderMap, HeaderValue, ALLOW, CACHE_CONTROL, CONNECTION, CONTENT_TYPE, SET_COOKIE},
    Method, Request, Response, StatusCode,
};
use tokio::sync::Notify;

use crate::app::Cassie;
use crate::app::CassieSession;
use crate::catalog::RoleMeta;
use crate::rest::static_files::AdminUiStaticFiles;

mod collection_routes;
mod request_execution;

use collection_routes::dispatch_collection_routes;
use request_execution::{
    run_rest_blocking_route, run_rest_blocking_route_controlled,
    run_rest_blocking_route_with_cancellation, RestBlockingError, RestRequestExecution,
};

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
    request_execution::run_server(addr, cassie, shutdown, admin_ui_dir).await
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
    query: Option<&'a str>,
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
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; connect-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data:; worker-src 'self' blob:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
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
    let query = request.uri().query().map(str::to_string);
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
            query: query.as_deref(),
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
        ("GET", ["api", "v1", "admin", "databases"]) => {
            let mut database_metadata = cassie
                .midge
                .list_databases()
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            database_metadata.sort_by_key(|database| database.name.to_ascii_lowercase());
            let databases = database_metadata
                .into_iter()
                .map(|database| {
                    serde_json::json!({
                        "name": database.name,
                        "description": database.description,
                    })
                })
                .collect::<Vec<_>>();
            Ok(Some(json_response(StatusCode::OK, &databases)))
        }
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
        ("DELETE", ["api", "v1", "admin", "query-operations", operation_id]) => {
            cancel_admin_query_operation(context, operation_id).await
        }
        ("GET", ["api", "v1", "admin", "query", "schema"] | ["api", "v1", "admin", "catalog"]) => {
            let database = query_database(context.query)?;
            let session = database_scoped_session(&cassie, context.request_context, &database)?;
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_admin_query_schema",
                move |cassie| crate::rest::query::schema_with_session(&cassie, &session),
            )
            .await
            .map(|value| Some(json_response(StatusCode::OK, &value)))
        }
        (
            "POST",
            ["api", "v1", "admin", "query", "execute"] | ["api", "v1", "admin", "query-executions"],
        ) => {
            run_admin_query_operation(
                context,
                cassie,
                body,
                "rest_admin_query_execute",
                |cassie, session, body, cancellation| {
                    crate::rest::query::execute_with_session_and_cancellation(
                        cassie,
                        session,
                        body,
                        cancellation,
                    )
                },
            )
            .await
        }
        (
            "POST",
            ["api", "v1", "admin", "query", "validate"]
            | ["api", "v1", "admin", "query-validations"],
        ) => {
            run_admin_query_operation(
                context,
                cassie,
                body,
                "rest_admin_query_validate",
                |cassie, session, body, cancellation| {
                    crate::rest::query::validate_with_session_and_cancellation(
                        cassie,
                        session,
                        body,
                        cancellation,
                    )
                },
            )
            .await
        }
        (
            "POST",
            ["api", "v1", "admin", "query", "explain"]
            | ["api", "v1", "admin", "query-explanations"],
        ) => {
            run_admin_query_operation(
                context,
                cassie,
                body,
                "rest_admin_query_explain",
                |cassie, session, body, cancellation| {
                    crate::rest::query::explain_with_session_and_cancellation(
                        cassie,
                        session,
                        body,
                        cancellation,
                    )
                },
            )
            .await
        }
        _ => Ok(None),
    }
}

async fn run_admin_query_operation<T>(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    body: &RestBytes,
    operation_name: &'static str,
    operation: impl FnOnce(
            &Cassie,
            &CassieSession,
            &[u8],
            &crate::runtime::QueryCancellationHandle,
        ) -> Result<T, crate::app::CassieError>
        + Send
        + 'static,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)>
where
    T: serde::Serialize + Send + 'static,
{
    let body = body.clone();
    let database = body_database(body.as_ref())?;
    let session = database_scoped_session(&cassie, context.request_context, &database)?;
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let registration = admin_query_operation_id(body.as_ref())?
        .map(|id| {
            crate::rest::query_operations::register(
                id,
                crate::rest::query_operations::owner_fingerprint(
                    context.request_context.token.as_deref(),
                ),
                cancellation.clone(),
            )
            .map_err(|_| {
                (
                    StatusCode::CONFLICT,
                    "query operation ID is already active".to_string(),
                )
            })
        })
        .transpose()?;
    let result = run_rest_blocking_route_with_cancellation(
        cassie,
        context.method,
        context.path,
        context.started_at,
        operation_name,
        cancellation,
        move |cassie, cancellation| operation(&cassie, &session, body.as_ref(), cancellation),
    )
    .await;
    if let Some(registration) = registration {
        registration.finish();
    }
    result.map(|value| Some(json_response(StatusCode::OK, &value)))
}

fn query_database(query: Option<&str>) -> Result<String, (StatusCode, String)> {
    let database = query
        .unwrap_or_default()
        .split('&')
        .find_map(|pair| {
            pair.split_once('=')
                .filter(|(key, _)| *key == "database")
                .map(|(_, value)| value)
        })
        .unwrap_or_default()
        .trim();
    required_database(database)
}

fn body_database(body: &[u8]) -> Result<String, (StatusCode, String)> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let database = value
        .get("database")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    required_database(database)
}

fn required_database(database: &str) -> Result<String, (StatusCode, String)> {
    let database = database.trim();
    if database.is_empty() {
        Err((StatusCode::BAD_REQUEST, "database is required".to_string()))
    } else {
        Ok(database.to_string())
    }
}

fn database_scoped_session(
    cassie: &Cassie,
    context: &RestRequestContext,
    database: &str,
) -> Result<CassieSession, (StatusCode, String)> {
    cassie.ensure_database_exists(database).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            format!("database '{database}' does not exist"),
        )
    })?;
    Ok(CassieSession::authenticated(
        context.session.user.clone(),
        Some(database.to_string()),
        context.role.as_ref().is_some_and(|role| role.is_admin),
    ))
}

async fn cancel_admin_query_operation(
    context: RouteDispatchContext<'_>,
    operation_id: &str,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    let operation_id = uuid::Uuid::parse_str(operation_id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "operation_id must be a valid UUID".to_string(),
        )
    })?;
    let owner =
        crate::rest::query_operations::owner_fingerprint(context.request_context.token.as_deref());
    match crate::rest::query_operations::cancel(operation_id, &owner, REST_REQUEST_TIMEOUT).await {
        Ok(()) => Ok(Some(json_response(
            StatusCode::OK,
            &serde_json::json!({"operation_id": operation_id, "cancelled": true}),
        ))),
        Err(crate::rest::query_operations::CancelError::NotFound) => Err((
            StatusCode::NOT_FOUND,
            "query operation was not found".to_string(),
        )),
        Err(crate::rest::query_operations::CancelError::AlreadyCompleted) => Err((
            StatusCode::CONFLICT,
            "query operation already completed".to_string(),
        )),
        Err(crate::rest::query_operations::CancelError::TimedOut) => Err((
            StatusCode::REQUEST_TIMEOUT,
            "query cancellation cleanup was not acknowledged".to_string(),
        )),
    }
}

fn admin_query_operation_id(body: &[u8]) -> Result<Option<uuid::Uuid>, (StatusCode, String)> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    value
        .get("operation_id")
        .map(|value| {
            let raw = value.as_str().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "operation_id must be a UUID string".to_string(),
                )
            })?;
            uuid::Uuid::parse_str(raw).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "operation_id must be a valid UUID".to_string(),
                )
            })
        })
        .transpose()
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
        ["api", "v1", "admin", "query-operations", _] => Some("DELETE"),
        _ => None,
    }
}

fn is_read_only_sql_route(method: &Method, segments: &[&str]) -> bool {
    matches!(
        (method.as_str(), segments),
        (
            "GET",
            ["api", "v1", "admin", "query", "schema"]
                | ["api", "v1", "admin", "catalog" | "databases"]
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
        ) | ("DELETE", ["api", "v1", "admin", "query-operations", _])
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

    RestRequestExecution::new(REST_REQUEST_TIMEOUT)
        .run_blocking(cassie, "rest_auth", move |cassie, _cancellation| {
            crate::rest::sessions::authenticate(&cassie, &token)
        })
        .await
        .map_err(|error| match error {
            RestBlockingError::Engine(crate::app::CassieError::Unauthorized) => {
                (StatusCode::UNAUTHORIZED, "unauthorized".to_string())
            }
            RestBlockingError::TimedOut => (
                StatusCode::REQUEST_TIMEOUT,
                "REST request timed out".to_string(),
            ),
            RestBlockingError::Engine(other) => (
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
mod tests;
