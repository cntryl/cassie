use hyper::{
    body::{Body, Incoming},
    header::{HeaderMap, HeaderValue, CACHE_CONTROL, CONTENT_TYPE},
    Method, Request, Response, StatusCode,
};

use crate::rest::body::RestBody;

pub(super) fn with_security_headers(
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

pub(super) fn is_api_path(raw_path: &str) -> bool {
    canonical_request_path(raw_path).is_ok_and(|path| path.starts_with("/api/"))
}

pub(super) fn canonical_request_path(raw_path: &str) -> Result<String, (StatusCode, String)> {
    let lowercase = raw_path.to_ascii_lowercase();
    let has_ambiguous_encoding = ["%2f", "%5c", "%2e"]
        .iter()
        .any(|encoding| lowercase.contains(encoding));
    let has_ambiguous_segment = raw_path
        .split('/')
        .any(|segment| matches!(segment, "." | ".."));
    if !raw_path.starts_with('/')
        || raw_path.starts_with("//")
        || raw_path.contains("//")
        || raw_path.contains('\\')
        || has_ambiguous_encoding
        || has_ambiguous_segment
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "ambiguous request path rejected".to_string(),
        ));
    }

    let path = raw_path.trim_end_matches('/');
    Ok(if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    })
}

pub(super) fn validate_rest_origin(
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

pub(super) fn validate_rest_content_type(
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

fn requires_json_content_type(method: &Method, path: &str) -> bool {
    path.starts_with("/api/") && matches!(method, &Method::POST | &Method::PUT | &Method::PATCH)
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
