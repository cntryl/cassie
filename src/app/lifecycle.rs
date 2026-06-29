use super::embeddings::build_embedding_provider;
use super::{Cassie, CassieError, CassieRuntimeConfig, Path, Arc, Midge, RuntimeState, Catalog, Mutex, BTreeMap, AtomicBool, Instant, Ordering};

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

        let schema_epoch = self.midge.schema_epoch();
        self.runtime
            .record_storage_access("schema", false, schema_epoch.is_ok());
        self.runtime.set_schema_epoch(
            schema_epoch
                .map_err(|error| CassieError::Storage(format!("load schema epoch: {error}")))?,
        );
        self.run_deferred_schema_cleanup()
            .map_err(|error| CassieError::Storage(format!("schema cleanup: {error}")))?;

        self.hydrate_catalog()
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.hydrate_runtime_feedback()?;
        self.runtime.mark_started();
        self.runtime.record_startup(started_at.elapsed());
        self.started.store(true, Ordering::SeqCst);
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
