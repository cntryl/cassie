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
        let data_dir =
            std::env::var("CASSIE_STORAGE_PATH").unwrap_or_else(|_| "./.cassie".to_string());
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
        Self::new_with_midge_and_config(midge, runtime_config)
    }

    fn new_with_midge_and_config(
        midge: Arc<Midge>,
        runtime_config: CassieRuntimeConfig,
    ) -> Result<Self, CassieError> {
        let embedding_provider = build_embedding_provider(&runtime_config)?;
        let bootstrap_password_hash = if runtime_config.password.is_empty() {
            None
        } else {
            Some(super::auth::hash_password(&runtime_config.password)?)
        };
        let dummy_password_hash = super::auth::hash_password(&uuid::Uuid::new_v4().to_string())?;
        let auth_rate_limiter = Arc::new(super::auth_rate_limit::AuthRateLimiter::new(
            runtime_config.auth_user_attempts_per_minute,
            runtime_config.auth_ip_attempts_per_minute,
            runtime_config.auth_rate_limit_max_entries,
        ));
        let CassieRuntimeConfig {
            database: default_database,
            password: auth_password,
            rest_tls_cert_file,
            rest_tls_key_file,
            rest_external_https,
            allow_insecure_non_loopback_listen,
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
            auth_user: "root".to_string(),
            auth_password,
            bootstrap_password_hash,
            dummy_password_hash,
            auth_rate_limiter,
            default_database,
            rest_tls_cert_file,
            rest_tls_key_file,
            rest_external_https,
            allow_insecure_non_loopback_listen,
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
        self.midge
            .reconcile_column_batch_indexes()
            .map_err(|error| CassieError::Storage(format!("column batch recovery: {error}")))?;
        self.midge
            .reconcile_fulltext_indexes()
            .map_err(|error| CassieError::Storage(format!("full-text recovery: {error}")))?;
        self.midge
            .reconcile_time_series_indexes()
            .map_err(|error| CassieError::Storage(format!("time-series recovery: {error}")))?;
        self.midge
            .reconcile_graph_adjacency()
            .map_err(|error| CassieError::Storage(format!("graph recovery: {error}")))?;
        self.midge
            .reconcile_ivfflat_indexes()
            .map_err(|error| CassieError::Storage(format!("IVFFlat recovery: {error}")))?;

        self.hydrate_catalog()
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.retry_materialized_projection_maintenance_debt()?;
        self.retry_rollup_maintenance_debt()?;
        self.hydrate_runtime_feedback()?;
        self.runtime.mark_started();
        self.runtime.record_startup(started_at.elapsed());
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn retry_rollup_maintenance_debt(&self) -> Result<(), CassieError> {
        let sources = self
            .midge
            .list_maintenance_debt()?
            .into_iter()
            .filter(|debt| debt.artifact == "rollup")
            .map(|debt| debt.collection)
            .collect::<std::collections::BTreeSet<_>>();
        let controls = self.runtime.query_controls(std::time::Instant::now());
        for source in sources {
            crate::executor::refresh_rollups_for_source_external(self, &source, &controls)
                .map_err(|error| CassieError::Execution(error.to_string()))?;
        }
        Ok(())
    }

    fn retry_materialized_projection_maintenance_debt(&self) -> Result<(), CassieError> {
        let debts = self
            .midge
            .list_maintenance_debt()?
            .into_iter()
            .filter(|debt| debt.artifact == "materialized_projection")
            .collect::<Vec<_>>();
        for debt in debts {
            let generation = self.midge.collection_generation(&debt.collection)?;
            if generation < debt.target_generation {
                continue;
            }
            crate::executor::mark_source_projections_stale_external(self, &debt.collection)
                .map_err(|error| CassieError::Execution(error.to_string()))?;
        }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::catalog::{IndexKind, IndexMeta};
    use crate::executor::filter::SearchContext;
    use crate::midge::adapter::{DocumentWriteBatchOptions, DocumentWriteOp, Midge};
    use crate::runtime::query_cache::{self, FulltextStatsCacheKey};
    use crate::types::{DataType, FieldSchema, Schema};

    use super::{Cassie, CassieRuntimeConfig};

    static INDEX_PUBLICATION_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn write_cloud_fixture(cassie: &Cassie, collection: &str) -> String {
        cassie
            .midge
            .create_collection(
                collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: false,
                    }],
                },
            )
            .expect("create cloud collection");
        cassie
            .midge
            .put_document(
                collection,
                Some("sync".to_string()),
                serde_json::json!({"title": "sync"}),
            )
            .expect("write synchronous document");
        cassie
            .midge
            .apply_document_write_batch_with_options(
                collection,
                vec![DocumentWriteOp::Put {
                    id: "buffered".to_string(),
                    payload: serde_json::json!({"title": "buffered"}),
                }],
                &DocumentWriteBatchOptions::buffered(cassie.midge.write_options_buffered()),
            )
            .expect("write buffered-intent document");
        let index = IndexMeta {
            collection: collection.to_string(),
            name: "idx_title".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD
            .lock()
            .expect("lock index publication failpoint");
        crate::midge::adapter::set_index_publication_failure_point(true);
        cassie
            .midge
            .put_index(&index)
            .expect_err("interrupt cloud index publication after its prepared commit");
        cassie
            .midge
            .replay_pending_index_publications()
            .expect("recover prepared cloud index publication");
        cassie
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .expect("write cloud maintenance metadata");
        let role = cassie
            .midge
            .get_role("root")
            .expect("load root role")
            .expect("root role");
        let token = crate::rest::sessions::issue(cassie, &role).expect("write cloud REST session");
        query_cache::store_fulltext_stats(
            &cassie.midge,
            &cassie.runtime,
            FulltextStatsCacheKey {
                collection,
                field: "title",
                analyzer_key: "standard",
                schema_epoch: 1,
                data_epoch: 1,
            },
            &SearchContext::default(),
        )
        .expect("write cloud query cache");
        token
    }

    fn assert_cloud_startup_and_mutations(strict: bool) {
        // Arrange
        let cache =
            std::env::temp_dir().join(format!("cassie-cloud-startup-{}", uuid::Uuid::new_v4()));
        let config = CassieRuntimeConfig::default();
        let midge = Arc::new(
            Midge::new_cloud_simulated_for_test(&cache, &config.database, strict)
                .expect("open simulated cloud"),
        );
        let cassie =
            Cassie::new_with_midge_and_config(Arc::clone(&midge), config).expect("create Cassie");

        // Act
        cassie.startup().expect("start cloud-backed Cassie");
        let collection = "public.cloud_documents";
        let token = write_cloud_fixture(&cassie, collection);

        // Assert
        assert!(cassie.is_started());
        assert!(cassie
            .midge
            .get_document(collection, "buffered")
            .expect("read buffered document")
            .is_some());
        assert!(cassie
            .midge
            .get_index(collection, "idx_title")
            .expect("read index")
            .is_some());
        assert_eq!(
            crate::rest::sessions::authenticate(&cassie, &token)
                .expect("authenticate cloud session")
                .session
                .user,
            "root"
        );

        drop(cassie);
        drop(midge);
        std::fs::remove_dir_all(cache).expect("remove cloud cache");
    }

    #[test]
    fn should_support_background_cloud_storage_lifecycle() {
        // Arrange / Act / Assert
        assert_cloud_startup_and_mutations(false);
    }

    #[test]
    fn should_support_strict_cloud_storage_lifecycle() {
        // Arrange / Act / Assert
        assert_cloud_startup_and_mutations(true);
    }
}
