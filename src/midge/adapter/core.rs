use super::native_memory::NativeMemoryConfiguration;
use super::{
    env, key_encoding, CassieError, ColumnFamilyHandle, Engine, Path, PublicationControlScope,
    Query, StorageFamily, StorageLayout, TransactionMode, WriteOptions,
};
use parking_lot::RwLock;
use parking_lot::{Mutex, ReentrantMutex};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

#[path = "storage_compatibility.rs"]
mod storage_compatibility;

pub struct Midge {
    pub(super) engine: Engine,
    write_policy: super::storage_config::WritePolicy,
    pub(super) native_memory_configuration: NativeMemoryConfiguration,
    compatibility_gate: Mutex<()>,
    bootstrap_gate: Mutex<()>,
    pub(super) storage_layout: OnceLock<StorageLayout>,
    pub(super) database_families: RwLock<BTreeMap<String, super::DatabaseFamily>>,
    pub(super) default_database: String,
    collection_write_gates: Mutex<HashMap<String, Arc<ReentrantMutex<()>>>>,
    referential_write_gate: ReentrantMutex<()>,
    pub(super) publication: super::publication::PublicationCoordinator,
    query_scan_entries: AtomicU64,
    column_validation_reads: AtomicU64,
    pub(super) column_batch_operational_metrics: Mutex<super::ColumnBatchOperationalMetrics>,
}

impl Drop for Midge {
    fn drop(&mut self) {
        if let Err(error) = self.engine.shutdown(Duration::from_secs(5)) {
            tracing::warn!(%error, "Midge graceful shutdown did not complete");
        }
    }
}

impl Midge {
    pub(crate) fn configure_repeatable_history_budget(
        &self,
        bytes: usize,
    ) -> Result<(), CassieError> {
        self.publication.configure_history_budget(bytes)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new() -> Result<Self, CassieError> {
        let data_dir = env::var("CASSIE_STORAGE_PATH").unwrap_or_else(|_| "./.cassie".to_string());
        Self::new_with_data_dir(data_dir)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        Self::new_with_data_dir_and_default_database(data_dir, "postgres")
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new_with_data_dir_and_default_database(
        data_dir: impl AsRef<Path>,
        default_database: impl Into<String>,
    ) -> Result<Self, CassieError> {
        let config = super::storage_config::open_config(data_dir.as_ref())?;
        Self::open_with_write_policy(config.open_options, default_database, config.write_policy)
    }

    pub(crate) fn new_in_memory(default_database: impl Into<String>) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::in_memory()
            .build()
            .map_err(CassieError::from)?;
        Self::open_with_write_policy(
            options,
            default_database,
            super::storage_config::WritePolicy::Local,
        )
    }

    pub(crate) fn new_in_memory_with_limits(
        default_database: impl Into<String>,
        budget: Option<usize>,
        memtable: Option<usize>,
    ) -> Result<Self, CassieError> {
        if budget.is_none() && memtable.is_none() {
            return Self::new_in_memory(default_database);
        }
        let options = super::native_memory::in_memory_options(budget, memtable)?;
        Self::open_with_write_policy(
            options,
            default_database,
            super::storage_config::WritePolicy::Local,
        )
    }

    fn open_with_write_policy(
        options: cntryl_midge::OpenOptions,
        default_database: impl Into<String>,
        write_policy: super::storage_config::WritePolicy,
    ) -> Result<Self, CassieError> {
        let native_memory_configuration = NativeMemoryConfiguration::from_options(&options);
        Ok(Self {
            engine: Engine::open(options).map_err(CassieError::from)?,
            native_memory_configuration,
            write_policy,
            compatibility_gate: Mutex::new(()),
            bootstrap_gate: Mutex::new(()),
            storage_layout: OnceLock::new(),
            database_families: RwLock::new(BTreeMap::new()),
            default_database: default_database.into(),
            collection_write_gates: Mutex::new(HashMap::new()),
            referential_write_gate: ReentrantMutex::new(()),
            publication: super::publication::PublicationCoordinator::default(),
            query_scan_entries: AtomicU64::new(0),
            column_validation_reads: AtomicU64::new(0),
            column_batch_operational_metrics: Mutex::new(
                super::ColumnBatchOperationalMetrics::default(),
            ),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_cloud_simulated_for_test(
        cache_path: impl AsRef<Path>,
        default_database: impl Into<String>,
        strict: bool,
    ) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::cloud_simulated(
            cache_path.as_ref(),
            "cassie-test",
            "startup",
        )
        .lease_ttl(std::time::Duration::from_secs(300))
        .build()
        .map_err(CassieError::from)?;
        let write_policy = if strict {
            super::storage_config::WritePolicy::CloudStrict
        } else {
            super::storage_config::WritePolicy::CloudBackground
        };
        Self::open_with_write_policy(options, default_database, write_policy)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new_strict_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        Self::new_strict_with_data_dir_and_default_database(data_dir, "postgres")
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn new_strict_with_data_dir_and_default_database(
        data_dir: impl AsRef<Path>,
        default_database: impl Into<String>,
    ) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref())
            .build()
            .map_err(CassieError::from)?;
        Self::open_with_write_policy(
            options,
            default_database,
            super::storage_config::WritePolicy::Local,
        )
    }

    #[must_use]
    pub(crate) fn write_options_sync(&self) -> WriteOptions {
        self.write_policy.sync()
    }

    #[must_use]
    pub(crate) fn write_options_buffered(&self) -> WriteOptions {
        self.write_policy.buffered()
    }

    #[doc(hidden)]
    #[must_use]
    pub fn query_scan_entries_for_diagnostics(&self) -> u64 {
        self.query_scan_entries.load(Ordering::Relaxed)
    }

    /// Field entries and owner-marker lookups performed by complete-source
    /// column ownership validation, separate from visited row markers.
    #[doc(hidden)]
    #[must_use]
    pub fn column_validation_reads_for_diagnostics(&self) -> u64 {
        self.column_validation_reads.load(Ordering::Relaxed)
    }

    pub(super) fn record_column_validation_read(&self) {
        self.column_validation_reads.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_query_scan_entry(&self) {
        self.query_scan_entries.fetch_add(1, Ordering::Relaxed);
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn bootstrap_families(&self) -> Result<StorageLayout, CassieError> {
        self.publication.ensure_recovered()?;
        self.ensure_families_ready()?;
        self.publication.ensure_recovered()?;
        self.storage_layout().ok_or_else(|| {
            CassieError::StorageBootstrap("failed to initialize midge storage families".to_string())
        })
    }

    fn bootstrap_families_inner(&self) -> Result<StorageLayout, CassieError> {
        self.check_existing_layout_admission()?;
        let schema = self.get_or_create_family(StorageFamily::Schema)?;
        let temp = self.get_or_create_family(StorageFamily::Temp)?;

        if schema.id() == temp.id() {
            return Err(CassieError::StorageBootstrap(
                "family ids must be distinct for schema/temp families".to_string(),
            ));
        }

        self.ensure_lexkey_layout_ready(&schema)?;
        self.replay_database_lifecycle_operations(&schema)?;
        let default_family = self.ensure_default_database(&schema)?;
        let database_families = self.load_database_families(&schema)?;
        *self.database_families.write() = database_families.clone();

        Ok(StorageLayout {
            schema,
            data: default_family.handle,
            temp,
            database_families: database_families
                .into_iter()
                .map(|(name, family)| (name, family.handle))
                .collect(),
        })
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn ensure_families_ready(&self) -> Result<&StorageLayout, CassieError> {
        if self.storage_layout.get().is_none() {
            #[cfg(test)]
            bootstrap_tests::before_bootstrap_gate();
            let _guard = self.bootstrap_gate.lock();
            if self.storage_layout.get().is_none() {
                #[cfg(test)]
                bootstrap_tests::initializing_bootstrap();
                let layout = self.bootstrap_families_inner()?;
                #[cfg(test)]
                if bootstrap_tests::fail_after_initialization() {
                    return Err(CassieError::StorageBootstrap(
                        "controlled failure after persisted initialization".to_owned(),
                    ));
                }
                let _ = self.storage_layout.set(layout);
            }
        }

        self.storage_layout.get().ok_or_else(|| {
            CassieError::StorageBootstrap("failed to initialize midge storage families".to_string())
        })
    }

    fn ensure_lexkey_layout_ready(&self, schema: &ColumnFamilyHandle) -> Result<(), CassieError> {
        let marker_key = key_encoding::layout_marker_key();
        let mut tx = self.begin_raw_adapter_tx(schema.id(), TransactionMode::ReadWrite)?;
        match tx.get(&marker_key).map_err(CassieError::from)? {
            Some(value) if Self::supports_storage_layout_marker(&value) => Ok(()),
            Some(value) => Err(Self::incompatible_layout_marker(&value)),
            None => {
                if let Some(family) = self.existing_unmarked_family()? {
                    return Err(Self::missing_layout_marker(&family));
                }
                tx.put(marker_key, key_encoding::LAYOUT_MARKER_VALUE.to_vec(), None)
                    .map_err(CassieError::from)?;
                tx.commit(self.write_options_sync())
                    .map_err(CassieError::from)
            }
        }
    }

    fn check_existing_layout_admission(&self) -> Result<(), CassieError> {
        self.reject_legacy_layout_prefixes()?;
        let marker = if let Some(schema) = self.engine.get_column_family(super::SCHEMA_FAMILY_NAME)
        {
            let read = self.begin_raw_adapter_tx(schema.id(), TransactionMode::ReadOnly)?;
            read.get(&key_encoding::layout_marker_key())
                .map_err(CassieError::from)?
        } else {
            None
        };
        match marker {
            Some(value) if Self::supports_storage_layout_marker(&value) => Ok(()),
            Some(value) => Err(Self::incompatible_layout_marker(&value)),
            None => match self.existing_unmarked_family()? {
                Some(family) => Err(Self::missing_layout_marker(&family)),
                None => Ok(()),
            },
        }
    }

    fn incompatible_layout_marker(value: &[u8]) -> CassieError {
        let version = String::from_utf8_lossy(value);
        let expected = String::from_utf8_lossy(key_encoding::LAYOUT_MARKER_VALUE);
        let upgraded = storage_compatibility::V020_LAYOUT_VERSION;
        CassieError::StorageBootstrap(format!(
            "incompatible Midge storage layout: found marker '{version}'; expected baseline marker '{expected}' or upgrade marker '{upgraded}'; preserve the data directory and use a compatible Cassie reader"
        ))
    }

    fn missing_layout_marker(family: &str) -> CassieError {
        CassieError::StorageBootstrap(format!(
            "missing persisted Midge storage layout marker: family '{family}' already exists or contains records; expected baseline marker '{}'; preserve the data directory and restore a validated backup",
            key_encoding::LAYOUT_VERSION
        ))
    }

    fn existing_unmarked_family(&self) -> Result<Option<String>, CassieError> {
        for family in self
            .engine
            .list_column_families()
            .map_err(CassieError::from)?
        {
            if !matches!(
                family.name(),
                super::SCHEMA_FAMILY_NAME | super::TEMP_FAMILY_NAME | super::DEFAULT_FAMILY_NAME
            ) {
                return Ok(Some(family.name().to_owned()));
            }
            let read = self.begin_raw_adapter_tx(family.id(), TransactionMode::ReadOnly)?;
            let entries = read
                .scan(&Query::new().limit(1))
                .map_err(CassieError::from)?
                .try_collect()
                .map_err(CassieError::from)?;
            if !entries.is_empty() {
                return Ok(Some(family.name().to_owned()));
            }
        }
        Ok(None)
    }

    fn reject_legacy_layout_prefixes(&self) -> Result<(), CassieError> {
        let families = self
            .engine
            .list_column_families()
            .map_err(CassieError::from)?;
        for family in families {
            let prefixes = if family.name() == super::SCHEMA_FAMILY_NAME {
                key_encoding::LEGACY_SCHEMA_PREFIXES
            } else if family.name() == super::TEMP_FAMILY_NAME {
                key_encoding::LEGACY_TEMP_PREFIXES
            } else {
                key_encoding::LEGACY_DATA_PREFIXES
            };
            let family_name = family.name();
            let tx = self.begin_raw_adapter_tx(family.id(), TransactionMode::ReadOnly)?;
            for prefix in prefixes {
                let scan = tx
                    .scan(&Query::new().prefix(prefix.to_vec().into()))
                    .map_err(CassieError::from)?
                    .try_collect()
                    .map_err(CassieError::from)?;
                if !scan.is_empty() {
                    return Err(CassieError::StorageBootstrap(format!(
                        "incompatible {} storage layout: found legacy key prefix '{}' in {family_name}; recreate the Midge data directory",
                        key_encoding::LAYOUT_VERSION,
                        String::from_utf8_lossy(prefix)
                    )));
                }
            }

            for prefix in key_encoding::legacy_layout_prefixes() {
                let mut scan = tx
                    .scan(&Query::new().prefix(prefix.into()))
                    .map_err(CassieError::from)?;
                if scan.next().is_some() {
                    return Err(CassieError::StorageBootstrap(format!(
                        "incompatible {} storage layout: found an older Cassie key baseline in {family_name}; recreate the Midge data directory",
                        key_encoding::LAYOUT_VERSION
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn storage_layout(&self) -> Option<StorageLayout> {
        let layout = self.storage_layout.get()?.clone();
        let families = self.database_families.read();
        Some(StorageLayout {
            data: families
                .get(&self.default_database.to_ascii_lowercase())
                .map_or(layout.data.clone(), |family| family.handle.clone()),
            database_families: families
                .iter()
                .map(|(name, family)| (name.clone(), family.handle.clone()))
                .collect(),
            ..layout
        })
    }

    pub(crate) fn with_transaction_write_gates<T>(
        &self,
        collections: &[String],
        referential: bool,
        controls: &crate::runtime::QueryExecutionControls,
        operation: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        super::collection_gates::with_controls(
            self,
            collections,
            referential.then_some(&self.referential_write_gate),
            controls,
            operation,
        )
    }

    /// Serializes a SQL sequence update through its catalog cursor publication.
    pub(crate) fn with_sequence_update_gate<T>(
        &self,
        sequence: &str,
        operation: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let canonical = self.canonical_collection_name(sequence);
        let sequences = [canonical];
        if let Some(controls) = PublicationControlScope::current_query_controls() {
            self.with_transaction_write_gates(&sequences, false, &controls, operation)
        } else {
            self.with_collection_gates(&sequences, operation)
        }
    }

    pub(crate) fn with_collection_write_gates<T>(
        &self,
        collections: &[String],
        operation: impl FnOnce() -> T,
    ) -> T {
        let _referential_guard = self.referential_write_gate.lock();
        self.with_collection_gates(collections, operation)
    }

    /// Holds only the per-collection write gates, without the global referential
    /// gate. Use it only for writes to collections outside any FOREIGN KEY.
    pub(crate) fn with_collection_gates<T>(
        &self,
        collections: &[String],
        operation: impl FnOnce() -> T,
    ) -> T {
        let mut names = collections
            .iter()
            .map(|collection| collection.to_ascii_lowercase())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        let gates = names
            .iter()
            .map(|collection| self.collection_write_gate(collection))
            .collect::<Vec<_>>();
        let _guards = gates.iter().map(|gate| gate.lock()).collect::<Vec<_>>();
        operation()
    }

    pub(crate) fn collection_write_gate(&self, collection: &str) -> Arc<ReentrantMutex<()>> {
        let mut gates = self.collection_write_gates.lock();
        gates
            .entry(collection.to_ascii_lowercase())
            .or_insert_with(|| Arc::new(ReentrantMutex::new(())))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use cntryl_midge::{OpenOptions, WriteOptions};

    use super::Midge;
    use crate::midge::adapter::storage_config::WritePolicy;

    #[test]
    fn should_bootstrap_a_fresh_cloud_backed_namespace() {
        // Arrange
        let cache =
            std::env::temp_dir().join(format!("cassie-cloud-bootstrap-{}", uuid::Uuid::new_v4()));
        let options = OpenOptions::cloud_simulated(&cache, "cassie-test", "bootstrap")
            .build()
            .expect("simulated cloud options");
        let midge =
            Midge::open_with_write_policy(options, "postgres", WritePolicy::CloudBackground)
                .expect("open simulated cloud");

        // Act
        let layout = midge
            .bootstrap_families()
            .expect("bootstrap cloud families");

        // Assert
        let schema_tx = midge
            .engine
            .begin_tx(layout.schema.id(), cntryl_midge::TransactionMode::ReadOnly)
            .expect("schema transaction");
        assert_eq!(
            schema_tx
                .get(&crate::midge::adapter::key_encoding::layout_marker_key())
                .expect("read layout marker"),
            Some(
                crate::midge::adapter::key_encoding::LAYOUT_MARKER_VALUE
                    .to_vec()
                    .into()
            )
        );
        assert_eq!(midge.write_options_sync(), WriteOptions::cloud_async());
        drop(midge);
        for attempt in 0..10 {
            match std::fs::remove_dir_all(&cache) {
                Ok(()) => break,
                Err(error)
                    if error.kind() == std::io::ErrorKind::DirectoryNotEmpty && attempt < 9 =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("remove cloud cache: {error}"),
            }
        }
    }
}

#[cfg(test)]
#[path = "core/bootstrap_tests.rs"]
mod bootstrap_tests;
