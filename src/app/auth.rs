use super::{
    normalize_role_name, Arc, Argon2, Cassie, CassieError, CassieSession, Mutex, OsRng,
    PasswordHash, PasswordHasher, PasswordVerifier, RoleMeta, SaltString,
};
use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;

#[cfg(test)]
thread_local! {
    static PASSWORD_HASH_GENERATION_COUNT: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
    static PASSWORD_VERIFICATION_COUNT: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[derive(Debug)]
pub(crate) struct AuthenticatedPrincipal {
    pub(crate) session: CassieSession,
    pub(crate) role: RoleMeta,
}

pub(super) fn hash_password(password: &str) -> Result<String, CassieError> {
    #[cfg(test)]
    PASSWORD_HASH_GENERATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| CassieError::Execution(format!("failed to hash role password: {error}")))
}

pub(super) fn verify_password(hash: &str, password: &str) -> Result<bool, CassieError> {
    #[cfg(test)]
    PASSWORD_VERIFICATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    let parsed = PasswordHash::new(hash)
        .map_err(|error| CassieError::Execution(format!("invalid password hash: {error}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[derive(Default)]
struct LazyPasswordHashState {
    hash: OnceLock<String>,
    initialization: Mutex<()>,
}

#[derive(Clone, Default)]
pub(crate) struct LazyPasswordHash(Arc<LazyPasswordHashState>);

impl LazyPasswordHash {
    fn get_or_initialize(&self, password: impl FnOnce() -> String) -> Result<&str, CassieError> {
        if let Some(hash) = self.0.hash.get() {
            return Ok(hash);
        }

        let _initialization = self.0.initialization.lock();
        if self.0.hash.get().is_none() {
            let generated = hash_password(&password())?;
            let _ = self.0.hash.set(generated);
        }
        self.0.hash.get().map(String::as_str).ok_or_else(|| {
            CassieError::Execution("failed to initialize authentication password hash".to_string())
        })
    }

    fn remember(&self, hash: &str) {
        if self.0.hash.get().is_some() {
            return;
        }

        let _initialization = self.0.initialization.lock();
        if self.0.hash.get().is_none() {
            let _ = self.0.hash.set(hash.to_owned());
        }
    }
}

struct PreparedAuthenticationHashes<'a> {
    bootstrap: Option<&'a str>,
    dummy: &'a str,
}

impl Cassie {
    #[must_use]
    pub(crate) fn authentication_enabled(&self) -> bool {
        !self.auth_password.is_empty()
    }

    pub(crate) fn validate_network_listener(
        &self,
        listener: SocketAddr,
    ) -> Result<(), CassieError> {
        crate::config::validate_listener_password(&self.auth_password, listener)?;

        let bootstrap_role = normalize_role_name(&self.auth_user);
        if self.lookup_role(&bootstrap_role)?.is_some_and(|role| {
            role.password_hash
                .as_deref()
                .is_none_or(|hash| hash.trim().is_empty())
        }) {
            return Err(
                crate::config::CassieRuntimeConfigError::PasswordlessBootstrapRole {
                    role: bootstrap_role,
                    listener: listener.to_string(),
                }
                .into(),
            );
        }
        Ok(())
    }

    pub(crate) fn validate_rest_network_listener(
        &self,
        listener: SocketAddr,
    ) -> Result<(), CassieError> {
        self.validate_network_listener(listener)?;
        crate::config::validate_rest_listener_transport(
            self.rest_tls_cert_file.as_deref(),
            self.rest_tls_key_file.as_deref(),
            self.allow_insecure_non_loopback_listen,
            listener,
        )?;
        Ok(())
    }

    pub(crate) fn authenticate_principal(
        &self,
        user: &str,
        password: Option<&str>,
        database: Option<String>,
    ) -> Result<AuthenticatedPrincipal, CassieError> {
        self.authenticate_principal_inner(user, password, database, true)
    }

    fn authenticate_principal_inner(
        &self,
        user: &str,
        password: Option<&str>,
        database: Option<String>,
        allow_passwordless: bool,
    ) -> Result<AuthenticatedPrincipal, CassieError> {
        let hashes = self.prepare_authentication_hashes()?;
        let normalized = normalize_role_name(user);
        if normalized.is_empty() {
            let _ = verify_password(hashes.dummy, password.unwrap_or(""));
            return Err(CassieError::Unauthorized);
        }

        if let Some(role) = self.lookup_role(&normalized)? {
            Self::validate_role_credentials(&role, password, allow_passwordless, hashes.dummy)?;
            let database = database.unwrap_or_else(|| self.default_database.clone());
            self.ensure_database_exists(&database)?;
            if !role.can_access_database(&database) {
                return Err(CassieError::InsufficientPrivilege);
            }
            let session =
                CassieSession::authenticated(role.name.clone(), Some(database), role.is_admin);
            return Ok(AuthenticatedPrincipal { session, role });
        }

        self.authenticate_bootstrap_admin(
            &normalized,
            password,
            database,
            allow_passwordless,
            &hashes,
        )
    }

    pub(crate) fn authenticate_network_principal(
        &self,
        user: &str,
        password: Option<&str>,
        database: Option<String>,
        peer_ip: IpAddr,
    ) -> Result<AuthenticatedPrincipal, CassieError> {
        let attempt = self.auth_rate_limiter.consume(user, peer_ip)?;
        let result = self.authenticate_principal_inner(user, password, database, false);
        if result.is_ok() {
            self.auth_rate_limiter.refund(&attempt);
        }
        result
    }

    fn authenticate_bootstrap_admin(
        &self,
        normalized_user: &str,
        password: Option<&str>,
        database: Option<String>,
        allow_passwordless: bool,
        hashes: &PreparedAuthenticationHashes<'_>,
    ) -> Result<AuthenticatedPrincipal, CassieError> {
        let bootstrap_user = normalize_role_name(&self.auth_user);
        if normalized_user != bootstrap_user {
            let _ = verify_password(hashes.dummy, password.unwrap_or(""));
            return Err(CassieError::Unauthorized);
        }

        if self.auth_password.is_empty() {
            let _ = verify_password(hashes.dummy, password.unwrap_or(""));
            if !allow_passwordless || password.is_some_and(|value| !value.is_empty()) {
                return Err(CassieError::Unauthorized);
            }
        } else if !hashes
            .bootstrap
            .is_some_and(|hash| verify_password(hash, password.unwrap_or("")).unwrap_or(false))
        {
            return Err(CassieError::Unauthorized);
        }

        let role = RoleMeta::bootstrap_admin(&self.auth_user, None);
        let database = database.unwrap_or_else(|| self.default_database.clone());
        self.ensure_database_exists(&database)?;
        let session = CassieSession::authenticated(role.name.clone(), Some(database), true);
        Ok(AuthenticatedPrincipal { session, role })
    }

    fn validate_role_credentials(
        role: &RoleMeta,
        password: Option<&str>,
        allow_passwordless: bool,
        dummy_password_hash: &str,
    ) -> Result<(), CassieError> {
        let hash = role
            .can_login
            .then_some(role.password_hash.as_deref())
            .flatten()
            .unwrap_or(dummy_password_hash);
        let verified = verify_password(hash, password.unwrap_or("")).unwrap_or(false);
        let passwordless_allowed = allow_passwordless
            && role.can_login
            && role.password_hash.is_none()
            && password.is_none_or(str::is_empty);
        if !passwordless_allowed && (!role.can_login || role.password_hash.is_none() || !verified) {
            return Err(CassieError::Unauthorized);
        }

        Ok(())
    }

    pub(super) fn initialize_bootstrap_password_hash(&self) -> Result<Option<&str>, CassieError> {
        if self.auth_password.is_empty() {
            return Ok(None);
        }
        self.bootstrap_password_hash
            .get_or_initialize(|| self.auth_password.clone())
            .map(Some)
    }

    pub(super) fn remember_bootstrap_password_hash(&self, hash: &str) {
        self.bootstrap_password_hash.remember(hash);
    }

    fn prepare_authentication_hashes(
        &self,
    ) -> Result<PreparedAuthenticationHashes<'_>, CassieError> {
        let dummy = self
            .dummy_password_hash
            .get_or_initialize(|| uuid::Uuid::new_v4().to_string())?;
        let bootstrap = self.initialize_bootstrap_password_hash()?;
        Ok(PreparedAuthenticationHashes { bootstrap, dummy })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PASSWORD: &str = "cassie-rest-listener-password";

    fn password_hash_generation_count() -> usize {
        PASSWORD_HASH_GENERATION_COUNT.with(std::cell::Cell::get)
    }

    fn reset_password_hash_generation_count() {
        PASSWORD_HASH_GENERATION_COUNT.with(|count| count.set(0));
    }

    fn test_path(label: &str) -> String {
        std::env::temp_dir()
            .join(format!("cassie-auth-{label}-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned()
    }

    fn cassie_with_config(
        label: &str,
        config: crate::config::CassieRuntimeConfig,
    ) -> (Cassie, String) {
        let path = std::env::temp_dir()
            .join(format!(
                "cassie-rest-listener-{label}-{}",
                uuid::Uuid::new_v4()
            ))
            .to_string_lossy()
            .to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        (cassie, path)
    }

    #[test]
    fn should_require_rest_tls_for_actual_non_loopback_listener() {
        // Arrange
        let config = crate::config::CassieRuntimeConfig {
            password: TEST_PASSWORD.to_string(),
            ..crate::config::CassieRuntimeConfig::default()
        };
        let (cassie, path) = cassie_with_config("requires-tls", config);
        let listener = "0.0.0.0:0".parse().expect("listener address");

        // Act
        let error = cassie
            .validate_rest_network_listener(listener)
            .expect_err("non-loopback REST listener should require TLS");

        // Assert
        assert!(error.to_string().contains("REST TLS is required"));
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_allow_password_authenticated_loopback_rest_listener_without_tls() {
        // Arrange
        let config = crate::config::CassieRuntimeConfig {
            password: TEST_PASSWORD.to_string(),
            ..crate::config::CassieRuntimeConfig::default()
        };
        let (cassie, path) = cassie_with_config("loopback", config);
        let listener = "127.0.0.1:0".parse().expect("listener address");

        // Act
        let validation = cassie.validate_rest_network_listener(listener);

        // Assert
        assert!(validation.is_ok());
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_explicit_insecure_non_loopback_rest_listener_override() {
        // Arrange
        let config = crate::config::CassieRuntimeConfig {
            password: TEST_PASSWORD.to_string(),
            allow_insecure_non_loopback_listen: true,
            ..crate::config::CassieRuntimeConfig::default()
        };
        let (cassie, path) = cassie_with_config("insecure-override", config);
        let listener = "0.0.0.0:0".parse().expect("listener address");

        // Act
        let validation = cassie.validate_rest_network_listener(listener);

        // Assert
        assert!(validation.is_ok());
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_defer_authentication_hashes_for_embedded_construction() {
        // Arrange
        let path = test_path("deferred-hashes");
        reset_password_hash_generation_count();

        // Act
        let cassie = Cassie::new_with_data_dir(&path).expect("embedded Cassie");
        let generated_hashes = password_hash_generation_count();
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);

        // Assert
        assert_eq!(generated_hashes, 0);
    }

    #[test]
    fn should_hash_fresh_bootstrap_credentials_once_during_startup() {
        // Arrange
        let path = test_path("startup-hash");
        reset_password_hash_generation_count();

        // Act
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let generated_hashes = password_hash_generation_count();
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);

        // Assert
        assert_eq!(generated_hashes, 1);
    }

    #[test]
    fn should_reuse_persisted_bootstrap_hash_during_restart_authentication() {
        // Arrange
        let path = test_path("restart-hash");
        let initial = Cassie::new_with_data_dir_and_config(
            &path,
            crate::config::CassieRuntimeConfig::default(),
        )
        .expect("initial Cassie");
        initial.startup().expect("initial startup");
        drop(initial);
        reset_password_hash_generation_count();

        // Act
        let restarted = Cassie::new_with_data_dir_and_config(
            &path,
            crate::config::CassieRuntimeConfig::default(),
        )
        .expect("restarted Cassie");
        restarted.startup().expect("restart");
        let generated_during_restart = password_hash_generation_count();
        let authentication = restarted.authenticate_principal("root", Some("postgres"), None);
        let generated_after_authentication = password_hash_generation_count();
        drop(restarted);
        let _ = std::fs::remove_dir_all(path);

        // Assert
        assert!(authentication.is_ok());
        assert_eq!(generated_during_restart, 0);
        assert_eq!(generated_after_authentication, 1);
    }

    #[test]
    fn should_hash_rotated_bootstrap_credentials_once_during_restart() {
        // Arrange
        let path = test_path("rotated-hash");
        let initial = Cassie::new_with_data_dir_and_config(
            &path,
            crate::config::CassieRuntimeConfig {
                password: "old-password".to_string(),
                ..crate::config::CassieRuntimeConfig::default()
            },
        )
        .expect("initial Cassie");
        initial.startup().expect("initial startup");
        drop(initial);
        reset_password_hash_generation_count();

        // Act
        let restarted = Cassie::new_with_data_dir_and_config(
            &path,
            crate::config::CassieRuntimeConfig {
                password: "new-password".to_string(),
                ..crate::config::CassieRuntimeConfig::default()
            },
        )
        .expect("restarted Cassie");
        restarted.startup().expect("rotated startup");
        let generated_during_restart = password_hash_generation_count();
        let old_authentication =
            restarted.authenticate_principal("root", Some("old-password"), None);
        let new_authentication =
            restarted.authenticate_principal("root", Some("new-password"), None);
        let generated_after_authentication = password_hash_generation_count();
        drop(restarted);
        let _ = std::fs::remove_dir_all(path);

        // Assert
        assert!(matches!(old_authentication, Err(CassieError::Unauthorized)));
        assert!(new_authentication.is_ok());
        assert_eq!(generated_during_restart, 1);
        assert_eq!(generated_after_authentication, 2);
    }

    #[test]
    fn should_share_lazy_authentication_hashes_across_cassie_clones() {
        // Arrange
        let path = test_path("shared-hashes");
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        let cloned = cassie.clone();
        reset_password_hash_generation_count();

        // Act
        let first = cassie.authenticate_principal("root", Some("wrong"), None);
        let generated_after_first = password_hash_generation_count();
        let second = cloned.authenticate_principal("root", Some("wrong"), None);
        let generated_after_second = password_hash_generation_count();
        drop(cloned);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);

        // Assert
        assert!(matches!(first, Err(CassieError::Unauthorized)));
        assert!(matches!(second, Err(CassieError::Unauthorized)));
        assert_eq!(generated_after_first, 2);
        assert_eq!(generated_after_second, 2);
    }

    #[test]
    fn should_verify_invalid_user_password_with_constant_cost() {
        // Arrange
        let (cassie, path) = cassie_with_config(
            "constant-cost-auth",
            crate::config::CassieRuntimeConfig::default(),
        );
        PASSWORD_VERIFICATION_COUNT.with(|count| count.set(0));

        // Act
        let known = cassie.authenticate_principal("root", Some("wrong"), None);
        let known_count = PASSWORD_VERIFICATION_COUNT.with(std::cell::Cell::get);
        PASSWORD_VERIFICATION_COUNT.with(|count| count.set(0));
        let unknown = cassie.authenticate_principal("missing-user", Some("wrong"), None);
        let unknown_count = PASSWORD_VERIFICATION_COUNT.with(std::cell::Cell::get);

        // Assert
        assert!(matches!(known, Err(CassieError::Unauthorized)));
        assert!(matches!(unknown, Err(CassieError::Unauthorized)));
        assert_eq!(known_count, 1);
        assert_eq!(unknown_count, 1);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}
