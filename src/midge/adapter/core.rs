use super::{
    env, key_encoding, CassieError, ColumnFamilyHandle, Engine, Path, Query, StorageFamily,
    StorageLayout, TransactionMode, WriteOptions,
};
use parking_lot::RwLock;
use parking_lot::{Mutex, ReentrantMutex};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub struct Midge {
    pub(super) engine: Engine,
    write_policy: super::storage_config::WritePolicy,
    pub(super) storage_layout: OnceLock<StorageLayout>,
    pub(super) database_families: RwLock<BTreeMap<String, super::DatabaseFamily>>,
    pub(super) default_database: String,
    collection_write_gates: Mutex<HashMap<String, Arc<ReentrantMutex<()>>>>,
    referential_write_gate: ReentrantMutex<()>,
    query_scan_entries: AtomicU64,
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

    fn open_with_write_policy(
        options: cntryl_midge::OpenOptions,
        default_database: impl Into<String>,
        write_policy: super::storage_config::WritePolicy,
    ) -> Result<Self, CassieError> {
        Ok(Self {
            engine: Engine::open(options).map_err(CassieError::from)?,
            write_policy,
            storage_layout: OnceLock::new(),
            database_families: RwLock::new(BTreeMap::new()),
            default_database: default_database.into(),
            collection_write_gates: Mutex::new(HashMap::new()),
            referential_write_gate: ReentrantMutex::new(()),
            query_scan_entries: AtomicU64::new(0),
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

    pub(super) fn record_query_scan_entry(&self) {
        self.query_scan_entries.fetch_add(1, Ordering::Relaxed);
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn bootstrap_families(&self) -> Result<StorageLayout, CassieError> {
        let schema = self.get_or_create_family(StorageFamily::Schema)?;
        let temp = self.get_or_create_family(StorageFamily::Temp)?;

        if schema.id() == temp.id() {
            return Err(CassieError::StorageBootstrap(
                "family ids must be distinct for schema/temp families".to_string(),
            ));
        }

        self.ensure_lexkey_layout_ready(&schema, &temp)?;
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
            let layout = self.bootstrap_families()?;
            let _ = self.storage_layout.set(layout);
        }

        self.storage_layout.get().ok_or_else(|| {
            CassieError::StorageBootstrap("failed to initialize midge storage families".to_string())
        })
    }

    fn ensure_lexkey_layout_ready(
        &self,
        schema: &ColumnFamilyHandle,
        temp: &ColumnFamilyHandle,
    ) -> Result<(), CassieError> {
        self.reject_legacy_layout_prefixes(schema, temp)?;

        let marker_key = key_encoding::layout_marker_key();
        let mut tx = self
            .engine
            .begin_tx(schema.id(), TransactionMode::ReadWrite)
            .map_err(CassieError::from)?;
        match tx.get(&marker_key).map_err(CassieError::from)? {
            Some(value) if value == key_encoding::LAYOUT_MARKER_VALUE => Ok(()),
            Some(value) => {
                let version = String::from_utf8_lossy(&value);
                let expected = String::from_utf8_lossy(key_encoding::LAYOUT_MARKER_VALUE);
                Err(CassieError::StorageBootstrap(format!(
                    "incompatible Midge storage layout: found marker '{version}'; expected baseline marker '{expected}'; recreate the Midge data directory"
                )))
            }
            None => {
                tx.put(marker_key, key_encoding::LAYOUT_MARKER_VALUE.to_vec(), None)
                    .map_err(CassieError::from)?;
                tx.commit(self.write_options_sync())
                    .map_err(CassieError::from)
            }
        }
    }

    fn reject_legacy_layout_prefixes(
        &self,
        schema: &ColumnFamilyHandle,
        temp: &ColumnFamilyHandle,
    ) -> Result<(), CassieError> {
        let families = self
            .engine
            .list_column_families()
            .map_err(CassieError::from)?;
        for family in families {
            let prefixes = if family.id() == schema.id() {
                key_encoding::LEGACY_SCHEMA_PREFIXES
            } else if family.id() == temp.id() {
                key_encoding::LEGACY_TEMP_PREFIXES
            } else {
                key_encoding::LEGACY_DATA_PREFIXES
            };
            let family_name = family.name();
            let tx = self
                .engine
                .begin_tx(family.id(), TransactionMode::ReadOnly)
                .map_err(CassieError::from)?;
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

    pub(crate) fn with_collection_write_gates<T>(
        &self,
        collections: &[String],
        operation: impl FnOnce() -> T,
    ) -> T {
        let _referential_guard = self.referential_write_gate.lock();
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
        std::fs::remove_dir_all(cache).expect("remove cloud cache");
    }
}
