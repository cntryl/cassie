use super::auth::hash_password;
use super::{normalize_role_name, Cassie, CassieError, CassieSession, CatalogObjectKind, RoleMeta};

impl Cassie {
    #[must_use]
    pub fn create_session(&self, user: &str, database: Option<String>) -> CassieSession {
        let database = database.or_else(|| Some(self.default_database.clone()));
        CassieSession::new(user.to_string(), database)
    }

    #[must_use]
    pub(crate) fn database_catalog_enforced(&self) -> bool {
        self.is_started() || !self.catalog.list_databases().is_empty()
    }

    pub(crate) fn ensure_database_exists(&self, database: &str) -> Result<(), CassieError> {
        if !self.database_catalog_enforced() || self.catalog.database_exists(database) {
            return Ok(());
        }

        if self.midge.get_database(database)?.is_some() {
            return Ok(());
        }

        Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Database,
            name: database.to_string(),
        })
    }

    pub(crate) fn ensure_session_database_exists(
        &self,
        session: &CassieSession,
    ) -> Result<(), CassieError> {
        let database = session
            .current_database()
            .unwrap_or(self.default_database.as_str());
        self.ensure_database_exists(database)
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
        self.authenticate_principal(user, password, database)
            .map(|principal| principal.session)
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
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Role,
                name: normalized,
            });
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
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Role,
                name: normalized,
            });
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
