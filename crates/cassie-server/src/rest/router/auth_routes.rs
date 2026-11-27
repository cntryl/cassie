use std::sync::Arc;

use hyper::{
    header::{HeaderMap, HeaderValue, SET_COOKIE},
    Method, Response, StatusCode,
};

use super::request_execution::{run_rest_blocking_route, RestBlockingError, RestRequestExecution};
use super::REST_REQUEST_TIMEOUT;
use super::{
    is_authentication_exempt, is_read_only_sql_route, is_route_public, is_self_service_auth_route,
    json_response, Cassie, RestBody, RestBytes, RestRequestContext, RouteDispatchContext,
};
use crate::rest::oidc::{self, Oidc};

pub(super) async fn dispatch_auth_routes(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    body: &RestBytes,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    match (context.method.as_str(), context.segments) {
        ("POST", ["api", "v1", "auth", "login"]) => {
            let body = body.clone();
            let peer_ip = context.peer_ip;
            run_rest_blocking_route(
                cassie,
                context.method,
                context.path,
                context.started_at,
                "rest_auth_login",
                move |cassie| crate::rest::sessions::login(&cassie, body.as_ref(), peer_ip),
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
                    "user": context.request_context.external_user.as_deref()
                        .unwrap_or(&context.request_context.session.user),
                    "role": context.request_context.role.as_ref().map(|role| role.name.clone()),
                }),
            )))
        }
        ("POST", ["api", "v1", "auth", "logout"]) => {
            if let Some(token) = context.request_context.token.as_deref() {
                if crate::rest::oidc::is_session_token(token) {
                    if let Some(oidc) = context.oidc {
                        oidc.revoke(token);
                    }
                } else {
                    revoke_password_session(context, cassie, token.to_owned()).await?;
                }
            }
            let mut response =
                json_response(StatusCode::OK, &serde_json::json!({"logged_out": true}));
            set_session_cookie(&mut response, "", true, context.secure_transport);
            Ok(Some(response))
        }
        _ => Ok(None),
    }
}

async fn revoke_password_session(
    context: RouteDispatchContext<'_>,
    cassie: Arc<Cassie>,
    token: String,
) -> Result<(), (StatusCode, String)> {
    run_rest_blocking_route(
        cassie,
        context.method,
        context.path,
        context.started_at,
        "rest_auth_logout",
        move |cassie| crate::rest::sessions::revoke(&cassie, &token),
    )
    .await
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

pub(super) async fn authenticate_request_context(
    cassie: &Arc<Cassie>,
    method: &Method,
    path: &str,
    segments: &[&str],
    headers: &HeaderMap,
    started_at: std::time::Instant,
    oidc: Option<&Oidc>,
) -> Result<RestRequestContext, (StatusCode, String)> {
    if is_route_public(method, segments)
        || is_authentication_exempt(method, segments)
        || !cassie.authentication_enabled()
    {
        return Ok(RestRequestContext {
            session: cassie.create_session(
                cassie.default_network_user(),
                Some(cassie.default_database().to_owned()),
            ),
            role: None,
            token: None,
            external_user: None,
        });
    }
    let report = |status: StatusCode| {
        cassie.runtime().record_rest_request(
            method.as_str(),
            path,
            status.as_u16(),
            started_at.elapsed(),
        );
    };
    let (principal, external_user) =
        match authenticate_rest_request(Arc::clone(cassie), headers, oidc).await {
            Ok(authenticated) => authenticated,
            Err((status, message)) => {
                report(status);
                return Err((status, message));
            }
        };
    if !principal.role.is_admin()
        && !is_read_only_sql_route(method, segments)
        && !is_self_service_auth_route(method, segments)
    {
        let status = StatusCode::FORBIDDEN;
        report(status);
        return Err((status, "forbidden".to_owned()));
    }
    Ok(RestRequestContext {
        session: principal.session,
        role: Some(principal.role),
        token: cookie_token(headers),
        external_user,
    })
}

async fn authenticate_rest_request(
    cassie: Arc<Cassie>,
    headers: &HeaderMap,
    oidc: Option<&Oidc>,
) -> Result<(crate::app::NetworkPrincipal, Option<String>), (StatusCode, String)> {
    let Some(token) = cookie_token(headers) else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };

    let external = if oidc::is_session_token(&token) {
        Some(
            oidc.and_then(|service| service.session(&token))
                .ok_or_else(|| (StatusCode::UNAUTHORIZED, "unauthorized".to_owned()))?,
        )
    } else {
        None
    };
    RestRequestExecution::new(REST_REQUEST_TIMEOUT)
        .run_blocking(cassie, "rest_auth", move |cassie, _cancellation| {
            if let Some(external) = external {
                let principal = cassie.external_principal(&external.authority)?;
                Ok((principal, Some(external.user)))
            } else {
                crate::rest::sessions::authenticate(&cassie, &token)
                    .map(|principal| (principal, None))
            }
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
