use super::{key_encoding, CassieError, DatabaseMeta, Midge, Query, WriteOptions};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn create_database(
        &self,
        name: &str,
        description: Option<String>,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let database_key = Self::database_key(name);
        if tx.get(&database_key).map_err(CassieError::from)?.is_none() {
            let metadata = DatabaseMeta::new(name, description);
            let value = serde_json::to_vec(&metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(database_key, value, None).map_err(CassieError::from)?;
        }

        let mut databases = Self::load_databases(&tx)?;
        if !databases.iter().any(|entry| entry == name) {
            databases.push(name.to_string());
            databases.sort();
            databases.dedup();
            Self::save_databases(&mut tx, &databases)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_database(&self, name: &str) -> Result<Option<DatabaseMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_database_from_tx(&tx, name)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_databases(&self) -> Result<Vec<DatabaseMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let mut names = Self::load_databases(&tx)?;
        if names.is_empty() {
            let scan = tx
                .scan(&Query::new().prefix(Self::database_prefix().into()))
                .map_err(CassieError::from)?;
            let prefix = Self::database_prefix();
            for (raw_key, raw_value) in scan {
                if let Ok(metadata) = serde_json::from_slice::<DatabaseMeta>(&raw_value) {
                    names.push(metadata.name);
                    continue;
                }
                if let Some(name) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) {
                    names.push(name);
                }
            }
        }

        names.sort();
        names.dedup();
        let mut databases = Vec::with_capacity(names.len());
        for name in names {
            if let Some(metadata) = Self::load_database_from_tx(&tx, &name)? {
                databases.push(metadata);
            }
        }
        databases.sort_by_key(|database| database.name.to_ascii_lowercase());
        Ok(databases)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn drop_database(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::database_key(name))
            .map_err(CassieError::from)?;
        let mut databases = Self::load_databases(&tx)?;
        databases.retain(|entry| entry != name);
        Self::save_databases(&mut tx, &databases)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn load_database_from_tx(
        tx: &cntryl_midge::Transaction,
        name: &str,
    ) -> Result<Option<DatabaseMeta>, CassieError> {
        let Some(raw) = tx
            .get(&Self::database_key(name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid database metadata: {error}")))
    }
}
