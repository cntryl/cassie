use super::embeddings::build_embedding_provider;
use super::{
    Arc, AtomicBool, BTreeMap, Cassie, CassieError, CassieRuntimeConfig, Catalog, Instant, Midge,
    Mutex, Ordering, Path, RuntimeState,
};
use crate::catalog::{canonical_schema_name, DEFAULT_SCHEMA};

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new() -> Result<Self, CassieError> {
        let data_dir = std::env::var("CASSIE_MIDGE_DATA_DIR")
            .unwrap_or_else(|_| "./.cassie/midge".to_string());
        Self::new_with_data_dir_and_config(data_dir, CassieRuntimeConfig::from_env()?)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        Self::new_with_data_dir_and_config(data_dir, CassieRuntimeConfig::from_env()?)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new_with_data_dir_and_config(
        data_dir: impl AsRef<Path>,
        runtime_config: CassieRuntimeConfig,
    ) -> Result<Self, CassieError> {
        let midge = Arc::new(Midge::new_with_data_dir_and_default_database(
            data_dir.as_ref(),
            &runtime_config.database,
        )?);
        let embedding_provider = build_embedding_provider(&runtime_config)?;
        let CassieRuntimeConfig {
            user: auth_user,
            database: default_database,
            password: auth_password,
            limits,
            ..
        } = runtime_config;
        let runtime = Arc::new(RuntimeState::new(limits));
        Ok(Self {
            midge,
            catalog: Catalog::new(),
            embedding_provider,
            runtime,
            normalized_vector_cache: Arc::new(Mutex::new(BTreeMap::new())),
            query_embedding_cache: Arc::new(Mutex::new(BTreeMap::new())),
            vector_search_result_cache: Arc::new(Mutex::new(BTreeMap::new())),
            auth_user,
            auth_password,
            default_database,
            started: Arc::new(AtomicBool::new(false)),
        })
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn startup(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        let families_ready = self.midge.ensure_families_ready();
        self.runtime
            .record_storage_access("schema", true, families_ready.is_ok());
        families_ready.map_err(|error| {
            CassieError::StorageBootstrap(format!("bootstrap families: {error}"))
        })?;
        self.bootstrap_default_database_if_empty()?;

        let schema_epoch = self.midge.schema_epoch();
        self.runtime
            .record_storage_access("schema", false, schema_epoch.is_ok());
        self.runtime.set_schema_epoch(
            schema_epoch
                .map_err(|error| CassieError::Storage(format!("load schema epoch: {error}")))?,
        );
        let data_epoch = self.midge.data_epoch();
        self.runtime
            .record_storage_access("data", false, data_epoch.is_ok());
        self.runtime.set_data_epoch(
            data_epoch
                .map_err(|error| CassieError::Storage(format!("load data epoch: {error}")))?,
        );
        self.run_deferred_schema_cleanup()
            .map_err(|error| CassieError::Storage(format!("schema cleanup: {error}")))?;
        self.midge
            .replay_pending_schema_operations()
            .map_err(|error| CassieError::Storage(format!("schema operation recovery: {error}")))?;
        self.midge
            .replay_pending_index_publications()
            .map_err(|error| {
                CassieError::Storage(format!("index publication recovery: {error}"))
            })?;
        self.midge
            .retry_maintenance_debt()
            .map_err(|error| CassieError::Storage(format!("maintenance recovery: {error}")))?;

        self.hydrate_catalog()
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.hydrate_runtime_feedback()?;
        self.runtime.mark_started();
        self.runtime.record_startup(started_at.elapsed());
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn bootstrap_default_database_if_empty(&self) -> Result<(), CassieError> {
        let databases = self
            .midge
            .list_databases()
            .map_err(|error| CassieError::Storage(format!("list databases: {error}")))?;
        if !databases.is_empty() {
            let public_schema = canonical_schema_name(&self.default_database, DEFAULT_SCHEMA);
            if !self
                .midge
                .list_namespaces_canonical()
                .iter()
                .any(|namespace| namespace.eq_ignore_ascii_case(&public_schema))
            {
                self.midge
                    .create_namespace(&public_schema)
                    .map_err(|error| {
                        CassieError::Storage(format!("bootstrap public schema: {error}"))
                    })?;
            }
            return Ok(());
        }

        if !self.midge.list_namespaces_canonical().is_empty()
            || !self.midge.list_collections().is_empty()
        {
            return Ok(());
        }

        self.midge
            .create_database(&self.default_database, None)
            .map_err(|error| CassieError::Storage(format!("bootstrap database: {error}")))?;
        self.midge
            .create_namespace(&canonical_schema_name(
                &self.default_database,
                DEFAULT_SCHEMA,
            ))
            .map_err(|error| CassieError::Storage(format!("bootstrap public schema: {error}")))?;
        Ok(())
    }

    #[must_use]
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub fn shutdown(&self) {
        if self.started.swap(false, Ordering::SeqCst) {
            self.runtime.record_shutdown();
            self.runtime.mark_shutdown();
        }
    }
}
