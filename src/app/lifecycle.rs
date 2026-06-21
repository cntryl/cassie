use super::auth::hash_password;
use super::embeddings::build_embedding_provider;
use super::*;

impl Cassie {
    pub fn new() -> Result<Self, CassieError> {
        let data_dir = std::env::var("CASSIE_MIDGE_DATA_DIR")
            .unwrap_or_else(|_| "./.cassie/midge".to_string());
        Self::new_with_data_dir_and_config(data_dir, CassieRuntimeConfig::from_env())
    }

    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        Self::new_with_data_dir_and_config(data_dir, CassieRuntimeConfig::from_env())
    }

    pub fn new_with_data_dir_and_config(
        data_dir: impl AsRef<Path>,
        runtime_config: CassieRuntimeConfig,
    ) -> Result<Self, CassieError> {
        let midge = Arc::new(Midge::new_with_data_dir(data_dir.as_ref())?);
        let embedding_provider = build_embedding_provider(&runtime_config)?;
        let runtime = Arc::new(RuntimeState::new(runtime_config.limits.clone()));
        let auth_user = runtime_config.user.clone();
        let auth_password = runtime_config.password.clone();
        let default_database = runtime_config.database.clone();
        Ok(Self {
            midge,
            catalog: Catalog::new(),
            embedding_provider,
            runtime,
            auth_user,
            auth_password,
            default_database,
            started: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn hydrate_catalog(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        self.catalog.clear();
        self.invalidate_plan_cache();

        let namespaces = self.midge.list_namespaces();
        self.runtime.record_storage_access("schema", false, true);
        for namespace in namespaces {
            self.catalog.register_namespace(&namespace, None);
        }

        let mut collections = self.midge.list_collections();
        self.runtime.record_storage_access("schema", false, true);
        if collections.is_empty() {
            collections = self.midge.list_collections_from_schema();
            self.runtime.record_storage_access("schema", false, true);
        }

        for name in collections {
            self.runtime.record_storage_access("schema", false, true);
            if let Some(schema) = self.midge.collection_schema(&name) {
                let constraints = self.midge.load_constraints(&name).map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!(
                        "load constraints for collection '{name}': {error}"
                    ))
                })?;
                self.runtime.record_storage_access("schema", false, true);
                self.catalog.register_collection_with_constraints(
                    &name,
                    schema
                        .fields
                        .into_iter()
                        .map(|field| (field.name, field.data_type))
                        .collect(),
                    constraints,
                );
                let projection_metadata = self
                    .midge
                    .projection_metadata(&name)?
                    .unwrap_or_else(|| crate::catalog::ProjectionMeta::new(&name, 1));
                self.catalog
                    .register_projection_metadata(projection_metadata);
            }
        }

        let indexes = self.midge.list_vector_indexes().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list vector indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_vector_index(index.clone());
            self.midge.rebuild_normalized_vectors_for_index(&index)?;
        }

        let indexes = self.midge.list_indexes().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_index(index);
        }

        for collection in self.catalog.list_collections() {
            self.hydrate_cardinality_stats(&collection.name)?;
        }

        let functions = self.midge.list_functions().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list functions: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in functions {
            self.catalog.register_function(metadata);
        }

        let procedures = self.midge.list_procedures().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list procedures: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in procedures {
            self.catalog.register_procedure(metadata);
        }

        let views = self.midge.list_views().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list views: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in views {
            self.catalog.register_view(metadata);
        }

        let rollups = self.midge.list_rollups().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list rollups: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in rollups {
            self.catalog.register_rollup(metadata);
        }

        let retention_policies = self.midge.list_retention_policies().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list retention policies: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in retention_policies {
            self.catalog.register_retention_policy(metadata);
        }

        self.hydrate_roles()?;
        self.runtime.record_catalog_hydration(started_at.elapsed());
        Ok(())
    }

    pub fn startup(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        let families_ready = self.midge.ensure_families_ready();
        self.runtime
            .record_storage_access("schema", true, families_ready.is_ok());
        families_ready.map_err(|error| {
            CassieError::StorageBootstrap(format!("bootstrap families: {error}"))
        })?;

        let schema_epoch = self.midge.schema_epoch();
        self.runtime
            .record_storage_access("schema", false, schema_epoch.is_ok());
        self.runtime.set_schema_epoch(
            schema_epoch
                .map_err(|error| CassieError::Storage(format!("load schema epoch: {error}")))?,
        );

        self.hydrate_catalog()
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.runtime.mark_started();
        self.runtime.record_startup(started_at.elapsed());
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub fn shutdown(&self) {
        if self.started.swap(false, Ordering::SeqCst) {
            self.runtime.record_shutdown();
            self.runtime.mark_shutdown();
        }
    }

    fn hydrate_roles(&self) -> Result<(), CassieError> {
        let mut roles = self.midge.list_roles().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list roles: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);

        let admin_name = normalize_role_name(&self.auth_user);
        if !roles.iter().any(|role| role.name == admin_name) {
            let password_hash = if self.auth_password.is_empty() {
                None
            } else {
                Some(hash_password(&self.auth_password)?)
            };
            let role = RoleMeta::bootstrap_admin(&self.auth_user, password_hash);
            self.midge.put_role(role.clone()).map_err(|error| {
                self.runtime.record_storage_access("schema", false, false);
                CassieError::Storage(format!("create bootstrap role: {error}"))
            })?;
            self.runtime.record_storage_access("schema", false, true);
            roles.push(role);
        }

        for role in roles {
            self.catalog.register_role(role);
        }

        Ok(())
    }

    fn hydrate_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        self.runtime.record_cardinality_read();
        match self.midge.get_cardinality_stats(collection) {
            Ok(Some(stats)) if stats.hydrated => {
                self.catalog.hydrate_cardinality_stats(collection, stats);
                Ok(())
            }
            Ok(_) => {
                self.runtime.record_cardinality_unavailable();
                let stats = self
                    .midge
                    .rebuild_cardinality_stats_for_collection(collection)
                    .map_err(|error| {
                        CassieError::Storage(format!(
                            "rebuild cardinality stats for collection '{collection}': {error}"
                        ))
                    })?;
                self.runtime.record_cardinality_rebuild();
                self.runtime.record_cardinality_write();
                self.catalog.hydrate_cardinality_stats(collection, stats);
                Ok(())
            }
            Err(error) => Err(CassieError::Storage(format!(
                "load cardinality stats for collection '{collection}': {error}"
            ))),
        }
    }

    pub(crate) fn refresh_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        let stats = self
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .map_err(|error| {
                CassieError::Storage(format!(
                    "rebuild cardinality stats for collection '{collection}': {error}"
                ))
            })?;
        self.runtime.record_cardinality_rebuild();
        self.runtime.record_cardinality_write();
        self.catalog.hydrate_cardinality_stats(collection, stats);
        Ok(())
    }
}
