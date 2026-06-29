use super::*;

pub struct Midge {
    pub(super) engine: Engine,
    pub(super) storage_layout: OnceLock<StorageLayout>,
}

impl Midge {
    pub fn new() -> Result<Self, CassieError> {
        let data_dir =
            env::var("CASSIE_MIDGE_DATA_DIR").unwrap_or_else(|_| "./.cassie/midge".to_string());
        Self::new_with_data_dir(data_dir)
    }

    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref()).build();

        let engine = match Engine::open(options) {
            Ok(engine) => engine,
            Err(error) => {
                if allow_memory_fallback() {
                    Engine::open(cntryl_midge::OpenOptions::in_memory().build())
                        .map_err(CassieError::from)?
                } else {
                    return Err(CassieError::from(error));
                }
            }
        };

        Ok(Self {
            engine,
            storage_layout: OnceLock::new(),
        })
    }

    pub fn new_strict_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref()).build();
        Ok(Self {
            engine: Engine::open(options).map_err(CassieError::from)?,
            storage_layout: OnceLock::new(),
        })
    }

    pub fn bootstrap_families(&self) -> Result<StorageLayout, CassieError> {
        let schema = self.get_or_create_family(StorageFamily::Schema)?;
        let data = self.get_or_create_family(StorageFamily::Data)?;
        let temp = self.get_or_create_family(StorageFamily::Temp)?;

        if schema.id() == data.id() || schema.id() == temp.id() || data.id() == temp.id() {
            return Err(CassieError::StorageBootstrap(
                "family ids must be distinct for schema/data/temp families".to_string(),
            ));
        }

        Ok(StorageLayout { schema, data, temp })
    }

    pub fn ensure_families_ready(&self) -> Result<&StorageLayout, CassieError> {
        if self.storage_layout.get().is_none() {
            let layout = self.bootstrap_families()?;
            self.ensure_lexkey_layout_ready(&layout)?;
            let _ = self.storage_layout.set(layout);
        }

        self.storage_layout.get().ok_or_else(|| {
            CassieError::StorageBootstrap("failed to initialize midge storage families".to_string())
        })
    }

    fn ensure_lexkey_layout_ready(&self, layout: &StorageLayout) -> Result<(), CassieError> {
        self.reject_legacy_layout_prefixes(layout)?;

        let marker_key = key_encoding::layout_marker_key();
        let mut tx = self
            .engine
            .begin_tx(layout.schema.id(), TransactionMode::ReadWrite)
            .map_err(CassieError::from)?;
        match tx.get(&marker_key).map_err(CassieError::from)? {
            Some(value) if value == key_encoding::LAYOUT_MARKER_VALUE => Ok(()),
            Some(value) => Err(CassieError::StorageBootstrap(format!(
                "incompatible lexkey v{} storage layout marker: {:?}",
                key_encoding::LAYOUT_VERSION,
                String::from_utf8_lossy(&value)
            ))),
            None => {
                tx.put(marker_key, key_encoding::LAYOUT_MARKER_VALUE.to_vec(), None)
                    .map_err(CassieError::from)?;
                tx.commit(WriteOptions::sync()).map_err(CassieError::from)
            }
        }
    }

    fn reject_legacy_layout_prefixes(&self, layout: &StorageLayout) -> Result<(), CassieError> {
        for (family_name, family_id, prefixes) in [
            (
                SCHEMA_FAMILY_NAME,
                layout.schema.id(),
                key_encoding::LEGACY_SCHEMA_PREFIXES,
            ),
            (
                DATA_FAMILY_NAME,
                layout.data.id(),
                key_encoding::LEGACY_DATA_PREFIXES,
            ),
            (
                TEMP_FAMILY_NAME,
                layout.temp.id(),
                key_encoding::LEGACY_TEMP_PREFIXES,
            ),
        ] {
            let tx = self
                .engine
                .begin_tx(family_id, TransactionMode::ReadOnly)
                .map_err(CassieError::from)?;
            for prefix in prefixes {
                let mut scan = tx
                    .scan(&Query::new().prefix(prefix.to_vec().into()))
                    .map_err(CassieError::from)?;
                if scan.next().is_some() {
                    return Err(CassieError::StorageBootstrap(format!(
                        "incompatible lexkey v{} storage layout: found v1 key prefix '{}' in {family_name}; recreate the Midge data directory",
                        key_encoding::LAYOUT_VERSION,
                        String::from_utf8_lossy(prefix)
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn storage_layout(&self) -> Option<StorageLayout> {
        self.storage_layout.get().cloned()
    }
}
