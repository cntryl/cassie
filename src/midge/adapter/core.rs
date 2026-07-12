use super::{
    allow_memory_fallback, env, key_encoding, CassieError, ColumnFamilyHandle, Engine, OnceLock,
    Path, Query, StorageFamily, StorageLayout, TransactionMode, WriteOptions,
};
use parking_lot::RwLock;
use parking_lot::{Mutex, ReentrantMutex};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

pub struct Midge {
    pub(super) engine: Engine,
    pub(super) storage_layout: OnceLock<StorageLayout>,
    pub(super) database_families: RwLock<BTreeMap<String, super::DatabaseFamily>>,
    pub(super) default_database: String,
    collection_write_gates: Mutex<HashMap<String, Arc<ReentrantMutex<()>>>>,
    referential_write_gate: ReentrantMutex<()>,
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
        let data_dir =
            env::var("CASSIE_MIDGE_DATA_DIR").unwrap_or_else(|_| "./.cassie/midge".to_string());
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
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref())
            .build()
            .map_err(CassieError::from)?;

        let engine = match Engine::open(options) {
            Ok(engine) => engine,
            Err(error) => {
                if allow_memory_fallback() {
                    Engine::open(
                        cntryl_midge::OpenOptions::in_memory()
                            .build()
                            .map_err(CassieError::from)?,
                    )
                    .map_err(CassieError::from)?
                } else {
                    return Err(CassieError::from(error));
                }
            }
        };

        Ok(Self {
            engine,
            storage_layout: OnceLock::new(),
            database_families: RwLock::new(BTreeMap::new()),
            default_database: default_database.into(),
            collection_write_gates: Mutex::new(HashMap::new()),
            referential_write_gate: ReentrantMutex::new(()),
        })
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
        Ok(Self {
            engine: Engine::open(options).map_err(CassieError::from)?,
            storage_layout: OnceLock::new(),
            database_families: RwLock::new(BTreeMap::new()),
            default_database: default_database.into(),
            collection_write_gates: Mutex::new(HashMap::new()),
            referential_write_gate: ReentrantMutex::new(()),
        })
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
                    "incompatible Midge storage layout: found marker '{version}'; expected lexkey v{} marker '{expected}'; recreate the Midge data directory",
                    key_encoding::LAYOUT_VERSION
                )))
            }
            None => {
                tx.put(marker_key, key_encoding::LAYOUT_MARKER_VALUE.to_vec(), None)
                    .map_err(CassieError::from)?;
                tx.commit(WriteOptions::sync()).map_err(CassieError::from)
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
                        "incompatible lexkey v{} storage layout: found legacy key prefix '{}' in {family_name}; recreate the Midge data directory",
                        key_encoding::LAYOUT_VERSION,
                        String::from_utf8_lossy(prefix)
                    )));
                }
            }

            let mut v2_scan = tx
                .scan(&Query::new().prefix(key_encoding::legacy_v2_layout_prefix().into()))
                .map_err(CassieError::from)?;
            if v2_scan.next().is_some() {
                return Err(CassieError::StorageBootstrap(format!(
                    "incompatible lexkey v{} storage layout: found v2 keys in {family_name}; recreate the Midge data directory",
                    key_encoding::LAYOUT_VERSION
                )));
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
