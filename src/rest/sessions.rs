use std::fmt::Write as _;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use cntryl_midge::{ConflictPolicy, Query, TransactionMode, WriteOptions};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::{AuthenticatedPrincipal, Cassie, CassieError, CassieSession};
use crate::catalog::{normalize_role_name, RoleMeta};
use crate::midge::StorageFamily;

pub(crate) const SESSION_COOKIE: &str = "cassie_session";
const SESSION_KEY_PREFIX: &[u8] = b"cassie.rest.session.";
const SESSION_QUOTA_REVISION_KEY: &[u8] = b"cassie.rest.sessions.quota-revision";
const SESSION_TTL_SECONDS: u64 = 8 * 60 * 60;
const MAX_SESSIONS: usize = 1_024;
const SESSION_COMMIT_ATTEMPTS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    user: String,
    #[serde(default)]
    database: Option<String>,
    expires_at: u64,
    credential_fingerprint: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    pub username: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
}

pub(crate) fn login(
    cassie: &Cassie,
    body: &[u8],
    peer_ip: IpAddr,
) -> Result<(String, AuthenticatedPrincipal), CassieError> {
    let request: LoginRequest = serde_json::from_slice(body)
        .map_err(|error| CassieError::Parse(format!("invalid login request: {error}")))?;
    let user = request
        .username
        .or(request.user)
        .ok_or(CassieError::Unauthorized)?;
    let principal =
        cassie.authenticate_network_principal(&user, request.password.as_deref(), None, peer_ip)?;
    let token = issue(cassie, &principal.role)?;
    Ok((token, principal))
}

pub(crate) fn issue(cassie: &Cassie, role: &RoleMeta) -> Result<String, CassieError> {
    issue_with_limits(cassie, role, MAX_SESSIONS, SESSION_TTL_SECONDS)
}

fn issue_with_limits(
    cassie: &Cassie,
    role: &RoleMeta,
    max_sessions: usize,
    ttl_seconds: u64,
) -> Result<String, CassieError> {
    let now = unix_seconds()?;
    let max_sessions_per_user = cassie
        .runtime
        .limits()
        .rest_max_sessions_per_user
        .min(max_sessions);
    for attempt in 0..SESSION_COMMIT_ATTEMPTS {
        match issue_transaction(
            cassie,
            role,
            max_sessions,
            max_sessions_per_user,
            ttl_seconds,
            now,
        ) {
            Err(CassieError::StorageRetryable(_)) if attempt + 1 < SESSION_COMMIT_ATTEMPTS => {}
            result => return result,
        }
    }
    unreachable!("REST session transaction retry loop always returns")
}

fn issue_transaction(
    cassie: &Cassie,
    role: &RoleMeta,
    max_sessions: usize,
    max_sessions_per_user: usize,
    ttl_seconds: u64,
    now: u64,
) -> Result<String, CassieError> {
    let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite)?;
    tx.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
    let revision = tx
        .get(SESSION_QUOTA_REVISION_KEY)
        .map_err(CassieError::from)?
        .map_or(Ok(0_u64), |value| {
            serde_json::from_slice(&value).map_err(|error| {
                CassieError::Parse(format!("invalid REST quota revision: {error}"))
            })
        })?;
    let entries = tx
        .scan(&Query::new().prefix(SESSION_KEY_PREFIX.to_vec().into()))
        .map_err(CassieError::from)?
        .try_collect()
        .map_err(CassieError::from)?;
    let normalized_user = normalize_role_name(&role.name);
    let mut active = 0_usize;
    let mut active_for_user = 0_usize;
    for (key, value) in entries {
        let Some(record) = decode_record(&value) else {
            tx.delete(key.to_vec()).map_err(CassieError::from)?;
            continue;
        };
        if record.expires_at <= now {
            tx.delete(key.to_vec()).map_err(CassieError::from)?;
            continue;
        }
        active = active.saturating_add(1);
        if normalize_role_name(record.user) == normalized_user {
            active_for_user = active_for_user.saturating_add(1);
        }
    }
    if active >= max_sessions {
        return Err(CassieError::Unsupported(
            "REST session capacity exhausted".to_string(),
        ));
    }
    if active_for_user >= max_sessions_per_user {
        return Err(CassieError::Unsupported(format!(
            "REST session quota exhausted for role '{normalized_user}'"
        )));
    }

    let token = random_token();
    let record = PersistedSession {
        user: normalized_user,
        database: None,
        expires_at: now.saturating_add(ttl_seconds),
        credential_fingerprint: credential_fingerprint(cassie, role),
    };
    let value = serde_json::to_vec(&record)
        .map_err(|error| CassieError::Execution(format!("encode REST session: {error}")))?;
    tx.put(session_key(&token), value, None)
        .map_err(CassieError::from)?;
    tx.put(
        SESSION_QUOTA_REVISION_KEY.to_vec(),
        serde_json::to_vec(&revision.saturating_add(1))
            .map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
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
    let role = match current_role(cassie, &record) {
        Ok(role) => role,
        Err(CassieError::Unauthorized) => {
            cassie.midge.raw_delete(StorageFamily::Schema, &key)?;
            return Err(CassieError::Unauthorized);
        }
        Err(error) => return Err(error),
    };
    let session = CassieSession::authenticated(role.name.clone(), None, role.is_admin);
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
    use std::sync::{Arc, Barrier};

    use super::{authenticate, issue, issue_with_limits, revoke, session_key, SESSION_TTL_SECONDS};
    use crate::app::{Cassie, CassieError};
    use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};

    fn cassie(label: &str) -> Cassie {
        cassie_with_config(label, CassieRuntimeConfig::default())
    }

    fn cassie_with_config(label: &str, config: CassieRuntimeConfig) -> Cassie {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        let path = std::env::temp_dir().join(format!(
            "cassie-rest-session-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        Cassie::new_with_data_dir_and_config(path, config).expect("cassie")
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
        let token = issue(&cassie, &role).expect("issue session");
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
        let token = issue(&cassie, &role).expect("issue session");

        // Act
        let principal = authenticate(&cassie, &token).expect("authenticate session");
        revoke(&cassie, &token).expect("revoke session");
        let revoked = authenticate(&cassie, &token).expect_err("revoked session");

        // Assert
        assert_eq!(principal.session.current_database(), None);
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
        let token = issue(&cassie, &role).expect("issue session");
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
        let token = issue(&cassie, &role).expect("issue session");
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
        let token = super::issue_with_limits(&cassie, &role, 1, 0).expect("issue expired session");

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
        issue_with_limits(&cassie, &role, 1, SESSION_TTL_SECONDS).expect("first session");

        // Act
        let result = issue_with_limits(&cassie, &role, 1, SESSION_TTL_SECONDS);

        // Assert
        assert!(matches!(
            result,
            Err(CassieError::Unsupported(message)) if message.contains("capacity exhausted")
        ));
    }

    #[test]
    fn should_atomically_enforce_the_global_session_quota_for_concurrent_issuers() {
        // Arrange
        let cassie = Arc::new(cassie("concurrent-global-cap"));
        let role = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("bootstrap role")
            .role;
        let barrier = Arc::new(Barrier::new(8));
        let issuers = (0..8)
            .map(|_| {
                let cassie = Arc::clone(&cassie);
                let role = role.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    issue_with_limits(&cassie, &role, 4, SESSION_TTL_SECONDS)
                })
            })
            .collect::<Vec<_>>();

        // Act
        let issued = issuers
            .into_iter()
            .flat_map(|issuer| issuer.join().expect("issuer"))
            .count();

        // Assert
        assert_eq!(issued, 4);
    }

    #[test]
    fn should_enforce_per_user_session_quotas_independently() {
        // Arrange
        let config = CassieRuntimeConfig {
            limits: CassieRuntimeLimits {
                rest_max_sessions_per_user: 1,
                ..CassieRuntimeLimits::default()
            },
            ..CassieRuntimeConfig::default()
        };
        let cassie = cassie_with_config("per-user-cap", config);
        cassie
            .create_role("reader", true, Some("reader-password".to_string()), false)
            .expect("reader role");
        let admin = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("admin role")
            .role;
        let reader = cassie
            .authenticate_principal("reader", Some("reader-password"), None)
            .expect("reader principal")
            .role;

        // Act
        let admin_token =
            issue_with_limits(&cassie, &admin, 4, SESSION_TTL_SECONDS).expect("admin session");
        let admin_overflow = issue_with_limits(&cassie, &admin, 4, SESSION_TTL_SECONDS);
        let reader_token =
            issue_with_limits(&cassie, &reader, 4, SESSION_TTL_SECONDS).expect("reader session");

        // Assert
        assert!(matches!(
            admin_overflow,
            Err(CassieError::Unsupported(message)) if message.contains("quota exhausted")
        ));
        assert!(authenticate(&cassie, &admin_token).is_ok());
        assert!(authenticate(&cassie, &reader_token).is_ok());
    }

    #[test]
    fn should_remove_expired_sessions_in_the_same_transaction_as_issuance() {
        // Arrange
        let cassie = cassie("atomic-expiry");
        let role = cassie
            .authenticate_principal("postgres", Some("postgres"), None)
            .expect("bootstrap role")
            .role;
        let expired = issue_with_limits(&cassie, &role, 1, 0).expect("expired session fixture");

        // Act
        let replacement =
            issue_with_limits(&cassie, &role, 1, SESSION_TTL_SECONDS).expect("replacement session");

        // Assert
        assert!(authenticate(&cassie, &expired).is_err());
        assert!(authenticate(&cassie, &replacement).is_ok());
    }
}
