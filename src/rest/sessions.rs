use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::{AuthenticatedPrincipal, Cassie, CassieError, CassieSession};
use crate::catalog::{normalize_role_name, RoleMeta};
use crate::midge::StorageFamily;

pub(crate) const SESSION_COOKIE: &str = "cassie_session";
const SESSION_KEY_PREFIX: &[u8] = b"cassie.rest.session.";
const SESSION_TTL_SECONDS: u64 = 8 * 60 * 60;
const MAX_SESSIONS: usize = 1_024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    user: String,
    database: String,
    expires_at: u64,
    credential_fingerprint: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    pub username: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
}

pub(crate) fn login(
    cassie: &Cassie,
    body: &[u8],
) -> Result<(String, AuthenticatedPrincipal), CassieError> {
    let request: LoginRequest = serde_json::from_slice(body)
        .map_err(|error| CassieError::Parse(format!("invalid login request: {error}")))?;
    let user = request
        .username
        .or(request.user)
        .ok_or(CassieError::Unauthorized)?;
    let principal =
        cassie.authenticate_principal(&user, request.password.as_deref(), request.database)?;
    let database = principal
        .session
        .current_database()
        .unwrap_or(cassie.default_database.as_str());
    let token = issue(cassie, &principal.role, database)?;
    Ok((token, principal))
}

pub(crate) fn issue(
    cassie: &Cassie,
    role: &RoleMeta,
    database: &str,
) -> Result<String, CassieError> {
    issue_with_limits(cassie, role, database, MAX_SESSIONS, SESSION_TTL_SECONDS)
}

fn issue_with_limits(
    cassie: &Cassie,
    role: &RoleMeta,
    database: &str,
    max_sessions: usize,
    ttl_seconds: u64,
) -> Result<String, CassieError> {
    cassie.ensure_database_exists(database)?;
    let now = unix_seconds()?;
    let mut active = 0;
    for (key, value) in session_entries(cassie)? {
        let Some(record) = decode_record(&value) else {
            cassie.midge.raw_delete(StorageFamily::Schema, &key)?;
            continue;
        };
        if record.expires_at <= now {
            cassie.midge.raw_delete(StorageFamily::Schema, &key)?;
        } else {
            active += 1;
        }
    }
    if active >= max_sessions {
        return Err(CassieError::Unsupported(
            "REST session capacity exhausted".to_string(),
        ));
    }

    let token = random_token();
    let record = PersistedSession {
        user: role.name.clone(),
        database: database.to_string(),
        expires_at: now.saturating_add(ttl_seconds),
        credential_fingerprint: credential_fingerprint(cassie, role),
    };
    let value = serde_json::to_vec(&record)
        .map_err(|error| CassieError::Execution(format!("encode REST session: {error}")))?;
    cassie
        .midge
        .raw_put(StorageFamily::Schema, &session_key(&token), &value)?;
    Ok(token)
}

pub(crate) fn authenticate(
    cassie: &Cassie,
    token: &str,
) -> Result<AuthenticatedPrincipal, CassieError> {
    let key = session_key(token);
    let Some(value) = cassie.midge.raw_get(StorageFamily::Schema, &key)? else {
        return Err(CassieError::Unauthorized);
    };
    let record = decode_record(&value).ok_or(CassieError::Unauthorized)?;
    if record.expires_at <= unix_seconds()? {
        cassie.midge.raw_delete(StorageFamily::Schema, &key)?;
        return Err(CassieError::Unauthorized);
    }
    cassie.ensure_database_exists(&record.database)?;
    let role = match current_role(cassie, &record) {
        Ok(role) => role,
        Err(CassieError::Unauthorized) => {
            cassie.midge.raw_delete(StorageFamily::Schema, &key)?;
            return Err(CassieError::Unauthorized);
        }
        Err(error) => return Err(error),
    };
    let session =
        CassieSession::authenticated(role.name.clone(), Some(record.database), role.is_admin);
    Ok(AuthenticatedPrincipal { session, role })
}

pub(crate) fn revoke(cassie: &Cassie, token: &str) -> Result<(), CassieError> {
    cassie
        .midge
        .raw_delete(StorageFamily::Schema, &session_key(token))
}

fn current_role(cassie: &Cassie, record: &PersistedSession) -> Result<RoleMeta, CassieError> {
    let normalized = normalize_role_name(&record.user);
    let role = cassie.lookup_role(&normalized)?.or_else(|| {
        (normalized == normalize_role_name(&cassie.auth_user))
            .then(|| RoleMeta::bootstrap_admin(&cassie.auth_user, None))
    });
    let Some(role) = role else {
        return Err(CassieError::Unauthorized);
    };
    if !role.can_login || credential_fingerprint(cassie, &role) != record.credential_fingerprint {
        return Err(CassieError::Unauthorized);
    }
    Ok(role)
}

fn credential_fingerprint(cassie: &Cassie, role: &RoleMeta) -> String {
    let credential = role
        .password_hash
        .as_deref()
        .filter(|_| !role.password_hash.as_deref().unwrap_or_default().is_empty())
        .unwrap_or_else(|| {
            if normalize_role_name(&role.name) == normalize_role_name(&cassie.auth_user) {
                cassie.auth_password.as_str()
            } else {
                ""
            }
        });
    digest(
        format!(
            "{}\0{}\0{}\0{}",
            role.name, role.can_login, role.is_admin, credential
        )
        .as_bytes(),
    )
}

type SessionEntries = Vec<(Vec<u8>, Vec<u8>)>;

fn session_entries(cassie: &Cassie) -> Result<SessionEntries, CassieError> {
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Schema, SESSION_KEY_PREFIX)
}

fn decode_record(value: &[u8]) -> Option<PersistedSession> {
    serde_json::from_slice(value).ok()
}

fn session_key(token: &str) -> Vec<u8> {
    let mut key = SESSION_KEY_PREFIX.to_vec();
    key.extend_from_slice(digest(token.as_bytes()).as_bytes());
    key
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    encode_hex(&bytes)
}

fn digest(value: &[u8]) -> String {
    encode_hex(&Sha256::digest(value))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

fn unix_seconds() -> Result<u64, CassieError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| CassieError::Execution(format!("system clock before epoch: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{authenticate, issue, issue_with_limits, revoke, session_key, SESSION_TTL_SECONDS};
    use crate::app::{Cassie, CassieError};

    fn cassie(label: &str) -> Cassie {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        let path = std::env::temp_dir().join(format!(
            "cassie-rest-session-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        Cassie::new_with_data_dir(path).expect("cassie")
    }

    #[test]
    fn should_issue_opaque_session_token_without_persisting_plaintext() {
        // Arrange
        let cassie = cassie("opaque");
        let role = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("bootstrap role")
            .role;

        // Act
        let token = issue(&cassie, &role, "cassie").expect("issue session");
        let entries = cassie
            .midge
            .raw_scan_prefix(crate::midge::StorageFamily::Schema, b"cassie.rest.session.")
            .expect("scan session records");

        // Assert
        assert_eq!(token.len(), 64);
        assert!(!entries
            .iter()
            .any(|(_, value)| { String::from_utf8_lossy(value).contains(token.as_str()) }));
    }

    #[test]
    fn should_revoke_authenticated_opaque_session() {
        // Arrange
        let cassie = cassie("revoke");
        let role = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("bootstrap role")
            .role;
        let token = issue(&cassie, &role, "cassie").expect("issue session");

        // Act
        let principal = authenticate(&cassie, &token).expect("authenticate session");
        revoke(&cassie, &token).expect("revoke session");
        let revoked = authenticate(&cassie, &token).expect_err("revoked session");

        // Assert
        assert_eq!(principal.session.current_database(), Some("cassie"));
        assert!(matches!(revoked, CassieError::Unauthorized));
    }

    #[test]
    fn should_reject_session_after_role_credential_rotation() {
        // Arrange
        let cassie = cassie("rotation");
        cassie
            .create_role("reader", true, Some("old-password".to_string()), false)
            .expect("create role");
        let role = cassie
            .authenticate_principal("reader", Some("old-password"), None)
            .expect("reader role")
            .role;
        let token = issue(&cassie, &role, "cassie").expect("issue session");
        cassie
            .execute_sql(
                &cassie.create_session("postgres", None),
                "ALTER ROLE reader PASSWORD 'new-password'",
                Vec::new(),
            )
            .expect("rotate password");

        // Act
        let error = authenticate(&cassie, &token).expect_err("rotated role invalidates session");

        // Assert
        assert!(matches!(error, CassieError::Unauthorized));
    }

    #[test]
    fn should_remove_session_after_role_deletion() {
        // Arrange
        let cassie = cassie("role-deletion");
        cassie
            .create_role("deleted-reader", true, Some("password".to_string()), false)
            .expect("create role");
        let role = cassie
            .authenticate_principal("deleted-reader", Some("password"), None)
            .expect("reader role")
            .role;
        let token = issue(&cassie, &role, "cassie").expect("issue session");
        cassie
            .drop_role("deleted-reader", false)
            .expect("drop role");

        // Act
        let error = authenticate(&cassie, &token).expect_err("deleted role is unauthorized");

        // Assert
        assert!(matches!(error, CassieError::Unauthorized));
        assert!(cassie
            .midge
            .raw_get(crate::midge::StorageFamily::Schema, &session_key(&token))
            .expect("session lookup")
            .is_none());
    }

    #[test]
    fn should_remove_expired_session_on_authentication() {
        // Arrange
        let cassie = cassie("expiry");
        let role = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("bootstrap role")
            .role;
        let token = super::issue_with_limits(&cassie, &role, "cassie", 1, 0)
            .expect("issue expired session");

        // Act
        let error = authenticate(&cassie, &token).expect_err("expired session");

        // Assert
        assert!(matches!(error, CassieError::Unauthorized));
        assert!(cassie
            .midge
            .raw_get(crate::midge::StorageFamily::Schema, &session_key(&token))
            .expect("session lookup")
            .is_none());
    }

    #[test]
    fn should_reject_a_new_session_when_active_cap_is_reached() {
        // Arrange
        let cassie = cassie("cap");
        let role = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("bootstrap role")
            .role;
        issue_with_limits(&cassie, &role, "cassie", 1, SESSION_TTL_SECONDS).expect("first session");

        // Act
        let result = issue_with_limits(&cassie, &role, "cassie", 1, SESSION_TTL_SECONDS);

        // Assert
        assert!(matches!(
            result,
            Err(CassieError::Unsupported(message)) if message.contains("capacity exhausted")
        ));
    }
}
