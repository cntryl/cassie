use super::auth::{hash_password, verify_password};
use super::{normalize_role_name, Cassie, CassieError, CassieSession, RoleMeta};

impl Cassie {
    #[must_use]
    pub fn create_session(&self, user: &str, database: Option<String>) -> CassieSession {
        let database = database.or_else(|| Some(self.default_database.clone()));
        CassieSession::new(user.to_string(), database)
    }

    pub(crate) fn lookup_role(&self, name: &str) -> Result<Option<RoleMeta>, CassieError> {
        let normalized = normalize_role_name(name);
        if normalized.is_empty() {
            return Ok(None);
        }

        if let Some(role) = self.catalog.get_role(&normalized) {
            return Ok(Some(role));
        }

        self.midge
            .get_role(&normalized)
            .map_err(|error| CassieError::Storage(format!("load role '{normalized}': {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn authenticate_role(
        &self,
        user: &str,
        password: Option<&str>,
        database: Option<String>,
    ) -> Result<CassieSession, CassieError> {
        let normalized = normalize_role_name(user);
        let Some(role) = self.lookup_role(&normalized)? else {
            return Err(CassieError::Unauthorized);
        };
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

        Ok(CassieSession::new(
            role.name,
            database.or_else(|| Some(self.default_database.clone())),
        ))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn create_role(
        &self,
        name: &str,
        login: bool,
        password: Option<String>,
        if_not_exists: bool,
    ) -> Result<(), CassieError> {
        let normalized = normalize_role_name(name);
        if normalized.is_empty() {
            return Err(CassieError::Planner(
                "CREATE ROLE requires a name".to_string(),
            ));
        }

        if self.lookup_role(&normalized)?.is_some() {
            if if_not_exists {
                return Ok(());
            }
            return Err(CassieError::Planner(format!(
                "role '{normalized}' already exists"
            )));
        }

        let password_hash = match (login, password) {
            (true, Some(password)) => Some(hash_password(&password)?),
            (true, None) => {
                return Err(CassieError::Planner(
                    "login roles require a password".into(),
                ));
            }
            (false, Some(_)) => {
                return Err(CassieError::Unsupported(
                    "PASSWORD is only supported for login roles".into(),
                ));
            }
            (false, None) => None,
        };

        let role = RoleMeta::new(normalized, login, false, password_hash);
        self.midge
            .put_role(&role)
            .map_err(|error| CassieError::Storage(format!("persist role '{name}': {error}")))?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn alter_role(
        &self,
        name: &str,
        login: Option<bool>,
        password: Option<String>,
    ) -> Result<(), CassieError> {
        let normalized = normalize_role_name(name);
        let Some(mut role) = self.lookup_role(&normalized)? else {
            return Err(CassieError::NotFound(format!(
                "role '{normalized}' not found"
            )));
        };

        if role.is_admin {
            if let Some(false) = login {
                return Err(CassieError::Unsupported(
                    "cannot disable the bootstrap admin role".into(),
                ));
            }
        }

        if let Some(login) = login {
            role.can_login = login;
        }

        if let Some(password) = password {
            role.password_hash = Some(hash_password(&password)?);
        }

        if role.can_login && role.password_hash.is_none() {
            return Err(CassieError::Planner(
                "login roles require a password".into(),
            ));
        }

        self.midge
            .put_role(&role)
            .map_err(|error| CassieError::Storage(format!("persist role '{name}': {error}")))?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn drop_role(&self, name: &str, if_exists: bool) -> Result<(), CassieError> {
        let normalized = normalize_role_name(name);
        let Some(role) = self.lookup_role(&normalized)? else {
            if if_exists {
                return Ok(());
            }
            return Err(CassieError::NotFound(format!(
                "role '{normalized}' not found"
            )));
        };

        if role.is_admin {
            return Err(CassieError::Unsupported(
                "cannot drop the bootstrap admin role".into(),
            ));
        }

        self.midge
            .delete_role(&normalized)
            .map_err(|error| CassieError::Storage(format!("delete role '{name}': {error}")))?;
        self.catalog.unregister_role(&normalized);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }
}
