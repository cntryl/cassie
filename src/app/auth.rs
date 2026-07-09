use super::{
    normalize_role_name, Argon2, Cassie, CassieError, CassieSession, OsRng, PasswordHash,
    PasswordHasher, PasswordVerifier, RoleMeta, SaltString,
};

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
            let session = CassieSession::new(role.name.clone(), Some(database));
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
        let session = CassieSession::new(role.name.clone(), Some(database));
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
