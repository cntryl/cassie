use std::net::IpAddr;

use serde::Deserialize;

use crate::app::{Cassie, CassieError, NetworkPrincipal};

pub(crate) const SESSION_COOKIE: &str = "cassie_session";

#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    pub username: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
}

/// Decodes the REST login body and delegates authentication and durable token
/// issuance to the engine's typed network-session operation.
pub(crate) fn login(
    cassie: &Cassie,
    body: &[u8],
    peer_ip: IpAddr,
) -> Result<(String, NetworkPrincipal), CassieError> {
    let request: LoginRequest = serde_json::from_slice(body)
        .map_err(|error| CassieError::Parse(format!("invalid login request: {error}")))?;
    let user = request
        .username
        .or(request.user)
        .ok_or(CassieError::Unauthorized)?;
    crate::app::network_sessions::login(cassie, &user, request.password.as_deref(), peer_ip)
}

pub(crate) fn authenticate(
    cassie: &Cassie,
    token: &str,
) -> Result<NetworkPrincipal, CassieError> {
    crate::app::network_sessions::authenticate(cassie, token)
}

pub(crate) fn revoke(cassie: &Cassie, token: &str) -> Result<(), CassieError> {
    crate::app::network_sessions::revoke(cassie, token)
}

#[cfg(test)]
#[path = "sessions/entrypoint_tests.rs"]
mod entrypoint_tests;
