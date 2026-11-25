use std::sync::Arc;

use hyper::{
    header::{HeaderValue, SET_COOKIE},
    Response, StatusCode,
};

use super::request_execution::run_rest_blocking_route;
use super::{json_response, Cassie, RestBody, RestBytes, RouteDispatchContext};

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
