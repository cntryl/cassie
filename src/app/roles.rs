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

    pub(crate) fn ensure_session_database_access(
        &self,
        session: &CassieSession,
    ) -> Result<(), CassieError> {
        self.ensure_session_database_exists(session)?;
        if !session.is_network_authenticated() {
            return Ok(());
        }
        let database = session
            .current_database()
            .unwrap_or(self.default_database.as_str());
        let role = self.lookup_role(&session.user)?.or_else(|| {
            (normalize_role_name(&session.user) == normalize_role_name(&self.auth_user))
                .then(|| RoleMeta::bootstrap_admin(&self.auth_user, None))
        });
        if role.is_some_and(|role| role.can_access_database(database)) {
            Ok(())
        } else {
            Err(CassieError::InsufficientPrivilege)
        }
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

        let mut role = RoleMeta::new(normalized, login, false, password_hash);
        role.grant_database(&self.default_database);
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

    /// Grant a non-admin role read and connection access to one database.
    ///
    /// # Errors
    ///
    /// Returns an error when the actor is not an administrator or persistence fails.
    pub fn grant_role_database_access(
        &self,
        actor: &CassieSession,
        role_name: &str,
        database: &str,
    ) -> Result<(), CassieError> {
        self.update_role_database_access(actor, role_name, database, true)
    }

    /// Revoke a non-admin role's read and connection access to one database.
    ///
    /// # Errors
    ///
    /// Returns an error when the actor is not an administrator or persistence fails.
    pub fn revoke_role_database_access(
        &self,
        actor: &CassieSession,
        role_name: &str,
        database: &str,
    ) -> Result<(), CassieError> {
        self.update_role_database_access(actor, role_name, database, false)
    }

    fn update_role_database_access(
        &self,
        actor: &CassieSession,
        role_name: &str,
        database: &str,
        grant: bool,
    ) -> Result<(), CassieError> {
        if !self
            .lookup_role(&actor.user)?
            .is_some_and(|role| role.is_admin)
        {
            return Err(CassieError::InsufficientPrivilege);
        }
        self.ensure_database_exists(database)?;
        let normalized = normalize_role_name(role_name);
        let Some(mut role) = self.lookup_role(&normalized)? else {
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Role,
                name: normalized,
            });
        };
        if role.is_admin {
            return Err(CassieError::Unsupported(
                "bootstrap administrators always have access to every database".to_string(),
            ));
        }
        if grant {
            role.grant_database(database);
        } else {
            role.revoke_database(database);
        }
        self.midge.put_role(&role).map_err(|error| {
            CassieError::Storage(format!("persist role database access: {error}"))
        })?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()
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
