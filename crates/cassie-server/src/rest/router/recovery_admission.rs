//! Storage-independent diagnostics and terminal REST admission after quarantine.

use std::sync::Arc;
use std::time::Instant;

use hyper::{Method, Response, StatusCode};

use super::{json_response, record_rest_error, Cassie, RestBody};

pub(super) fn guard(
    cassie: &Arc<Cassie>,
    method: &Method,
    path: &str,
    started_at: Instant,
) -> Result<Option<Response<RestBody>>, (StatusCode, String)> {
    let Err(error) = cassie.check_storage_recovery_required() else {
        return Ok(None);
    };
    if *method == Method::GET {
        let response = match path {
            "/health" => Some(json_response(StatusCode::OK, &cassie.health())),
            "/healthz" | "/readyz" | "/startupz" => Some(json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                &cassie.health(),
            )),
            "/liveness" | "/livez" | "/targetz" => Some(json_response(
                StatusCode::OK,
                &serde_json::json!({"ready": true}),
            )),
            _ => None,
        };
        if response.is_some() {
            return Ok(response);
        }
    }
    Err(record_rest_error(
        cassie,
        method.as_str(),
        path,
        started_at,
        &error,
    ))
}

#[cfg(test)]
mod tests;
