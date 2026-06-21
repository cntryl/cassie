use super::auth::{hash_password, verify_password};
use super::*;

impl Cassie {
    pub fn register_collection(&self, name: impl Into<String>, schema: crate::types::Schema) {
        let name = name.into();
        self.catalog.register_collection(
            &name,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        self.invalidate_plan_cache();
    }

    pub fn register_vector_index(&self, index: VectorIndexRecord) {
        self.catalog.register_vector_index(index);
        self.invalidate_plan_cache();
    }

    pub fn health(&self) -> serde_json::Value {
        let ready = self.is_started();
        let collections = self.midge.list_collections();
        serde_json::json!({
            "status": if ready { "ok" } else { "starting" },
            "ready": ready,
            "collections": collections.len(),
            "version": env!("CARGO_PKG_VERSION")
        })
    }

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
            .put_role(role.clone())
            .map_err(|error| CassieError::Storage(format!("persist role '{name}': {error}")))?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

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
            .put_role(role.clone())
            .map_err(|error| CassieError::Storage(format!("persist role '{name}': {error}")))?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

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

    pub fn metrics(&self) -> serde_json::Value {
        let snapshot = self.runtime.snapshot();
        serde_json::json!({
            "uptime_seconds": snapshot.runtime.uptime_seconds,
            "running_queries": snapshot.runtime.running_queries,
            "ready": self.is_started(),
            "auth_user": &self.auth_user,
            "runtime": snapshot.runtime,
            "query": snapshot.query,
            "rest": snapshot.rest,
            "pgwire": snapshot.pgwire,
            "search": snapshot.search,
            "vector": snapshot.vector,
            "hybrid": snapshot.hybrid,
            "storage": snapshot.storage,
            "plan_cache": snapshot.plan_cache,
            "query_cache": snapshot.query_cache,
            "cardinality": snapshot.cardinality,
            "feedback": snapshot.feedback,
            "adaptive_candidates": snapshot.adaptive_candidates,
            "covering_indexes": snapshot.covering_indexes,
            "parallel_scans": snapshot.parallel_scans,
            "parallel_scoring": snapshot.parallel_scoring,
            "parallel_aggregation": snapshot.parallel_aggregation,
        })
    }

    pub(crate) fn invalidate_plan_cache(&self) {
        self.runtime.invalidate_plan_cache();
    }

    pub(crate) fn bump_schema_epoch_and_invalidate_query_cache(&self) -> Result<(), CassieError> {
        let schema_epoch = self
            .midge
            .bump_schema_epoch()
            .map_err(|error| CassieError::Storage(format!("bump schema epoch: {error}")))?;
        self.runtime.record_storage_access("schema", true, true);
        self.runtime.set_schema_epoch(schema_epoch);
        self.runtime.invalidate_plan_cache();
        Ok(())
    }
}
