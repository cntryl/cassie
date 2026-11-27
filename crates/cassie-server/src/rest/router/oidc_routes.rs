//! Public browser endpoints for beginning and finishing OpenID Connect login.

use hyper::header::{HeaderMap, HeaderValue, CACHE_CONTROL, COOKIE, LOCATION, SET_COOKIE};
use hyper::{Method, Response, StatusCode};

use crate::rest::oidc::{Oidc, BROWSER_COOKIE, CHALLENGE_TTL};

use super::{full_body, json_response, Cassie, RestBody};

const FAILURE: &str = "/login?oidc_error=1";

pub(super) async fn dispatch(
    method: &Method,
    segments: &[&str],
    query: &str,
    headers: &HeaderMap,
    cassie: &Cassie,
    oidc: Option<&Oidc>,
    secure: bool,
) -> Option<Response<RestBody>> {
    if method != Method::GET {
        return None;
    }
    let oidc = oidc.filter(|_| secure && cassie.authentication_enabled());
    match segments {
        ["api", "v1", "auth", "methods"] => Some(no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"oidc": oidc.is_some()}),
        ))),
        ["api", "v1", "auth", "oidc", "start"] => {
            let mut response = redirect(FAILURE);
            if let Some(oidc) = oidc {
                let next = param(query, "next").unwrap_or_default();
                if let Ok(start) = oidc.start(&next).await {
                    response = redirect(start.location.as_str());
                    browser_cookie(&mut response, &start.browser, CHALLENGE_TTL);
                }
            }
            Some(response)
        }
        ["api", "v1", "auth", "oidc", "callback"] => {
            let mut response = redirect(FAILURE);
            if let (Some(oidc), Some(state), Some(browser)) =
                (oidc, param(query, "state"), cookie(headers, BROWSER_COOKIE))
            {
                let provider_error =
                    url::form_urlencoded::parse(query.as_bytes()).any(|(name, _)| name == "error");
                let code = if provider_error {
                    String::new()
                } else {
                    param(query, "code").unwrap_or_default()
                };
                if let Ok(session) = oidc.complete(cassie, &state, browser, &code).await {
                    response = redirect(&session.location);
                    let session_cookie = format!(
                        "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax; Secure",
                        crate::rest::sessions::SESSION_COOKIE,
                        session.token,
                        session.max_age,
                    );
                    append_cookie(&mut response, &session_cookie);
                    browser_cookie(&mut response, "", 0);
                }
            }
            Some(response)
        }
        _ => None,
    }
}

fn param(query: &str, name: &str) -> Option<String> {
    let mut matching = url::form_urlencoded::parse(query.as_bytes()).filter(|(key, _)| key == name);
    let (_, named) = matching.next()?;
    matching.next().is_none().then(|| named.into_owned())
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut matching = headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .filter_map(|(cookie, value)| (cookie == name).then_some(value));
    let value = matching.next()?;
    matching.next().is_none().then_some(value)
}

fn browser_cookie(response: &mut Response<RestBody>, value: &str, expires: u64) {
    let cookie = format!(
        "{BROWSER_COOKIE}={value}; Path=/; Max-Age={expires}; HttpOnly; SameSite=Lax; Secure"
    );
    append_cookie(response, &cookie);
}

fn append_cookie(response: &mut Response<RestBody>, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().append(SET_COOKIE, value);
    }
}

fn redirect(location: &str) -> Response<RestBody> {
    let mut response = Response::new(full_body(Vec::new()));
    *response.status_mut() = StatusCode::SEE_OTHER;
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(location).unwrap_or_else(|_| HeaderValue::from_static(FAILURE)),
    );
    no_store(response)
}

fn no_store(mut response: Response<RestBody>) -> Response<RestBody> {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
