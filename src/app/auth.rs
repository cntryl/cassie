use super::{
    normalize_role_name, Argon2, Cassie, CassieError, CassieSession, OsRng, PasswordHash,
    PasswordHasher, PasswordVerifier, RoleMeta, SaltString,
};
use std::net::SocketAddr;

#[derive(Debug)]
pub(crate) struct AuthenticatedPrincipal {
    pub(crate) session: CassieSession,
    pub(crate) role: RoleMeta,
}

pub(super) fn hash_password(password: &str) -> Result<String, CassieError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| CassieError::Execution(format!("failed to hash role password: {error}")))
}

pub(super) fn verify_password(hash: &str, password: &str) -> Result<bool, CassieError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|error| CassieError::Execution(format!("invalid password hash: {error}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
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
        let normalized = normalize_role_name(user);
        if normalized.is_empty() {
            return Err(CassieError::Unauthorized);
        }

        if let Some(role) = self.lookup_role(&normalized)? {
            validate_role_credentials(&role, password)?;
            let database = database.unwrap_or_else(|| self.default_database.clone());
            self.ensure_database_exists(&database)?;
            let session =
                CassieSession::authenticated(role.name.clone(), Some(database), role.is_admin);
            return Ok(AuthenticatedPrincipal { session, role });
        }

        self.authenticate_bootstrap_admin(&normalized, password, database)
    }

    fn authenticate_bootstrap_admin(
        &self,
        normalized_user: &str,
        password: Option<&str>,
        database: Option<String>,
    ) -> Result<AuthenticatedPrincipal, CassieError> {
        let bootstrap_user = normalize_role_name(&self.auth_user);
        if normalized_user != bootstrap_user {
            return Err(CassieError::Unauthorized);
        }

        if self.auth_password.is_empty() {
            if password.is_some_and(|value| !value.is_empty()) {
                return Err(CassieError::Unauthorized);
            }
        } else if password != Some(self.auth_password.as_str()) {
            return Err(CassieError::Unauthorized);
        }

        let role = RoleMeta::bootstrap_admin(&self.auth_user, None);
        let database = database.unwrap_or_else(|| self.default_database.clone());
        self.ensure_database_exists(&database)?;
        let session = CassieSession::authenticated(role.name.clone(), Some(database), true);
        Ok(AuthenticatedPrincipal { session, role })
    }
}

fn validate_role_credentials(role: &RoleMeta, password: Option<&str>) -> Result<(), CassieError> {
    if !role.can_login {
        return Err(CassieError::Unauthorized);
    }

    if let Some(hash) = role.password_hash.as_deref() {
        let Some(password) = password else {
            return Err(CassieError::Unauthorized);
        };
        if !verify_password(hash, password)? {
            return Err(CassieError::Unauthorized);
        }
    } else if password.is_some_and(|value| !value.is_empty()) {
        return Err(CassieError::Unauthorized);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PASSWORD: &str = "cassie-rest-listener-password";

    fn cassie_with_config(
        label: &str,
        config: crate::config::CassieRuntimeConfig,
    ) -> (Cassie, String) {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
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
}
