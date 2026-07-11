use super::{
    CassieError, Midge, Query, RawStorageEntry, StorageFamily, TransactionMode, WriteOptions,
};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn raw_get(
        &self,
        family: StorageFamily,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, CassieError> {
        let tx = self.transaction(family, TransactionMode::ReadOnly)?;
        let value = tx.get(key).map_err(CassieError::from)?;
        Ok(value.map(|value| value.to_vec()))
    }

    /// # Errors
    ///
    /// Returns an error when the database family cannot be opened.
    pub fn raw_get_database(
        &self,
        database: &str,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, CassieError> {
        let tx = self.database_tx(database, TransactionMode::ReadOnly)?;
        let value = tx.get(key).map_err(CassieError::from)?;
        Ok(value.map(|value| value.to_vec()))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn raw_scan_prefix(
        &self,
        family: StorageFamily,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let tx = self.transaction(family, TransactionMode::ReadOnly)?;
        let iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        let mut values = Vec::new();
        for (key, value) in iterator {
            values.push((key, value));
        }
        Ok(values)
    }

    /// # Errors
    ///
    /// Returns an error when the database family cannot be opened.
    pub fn raw_scan_prefix_database(
        &self,
        database: &str,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let tx = self.database_tx(database, TransactionMode::ReadOnly)?;
        let iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        Ok(iterator.collect())
    }

    pub(crate) fn raw_scan_database_page(
        &self,
        database: &str,
        prefix: &[u8],
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let mut query = Query::new()
            .prefix(prefix.to_vec().into())
            .limit(limit.saturating_add(1));
        if let Some(after) = after {
            query = query.start_key(after.to_vec().into());
        }
        let tx = self.database_tx(database, TransactionMode::ReadOnly)?;
        let iterator = tx.scan(&query).map_err(CassieError::from)?;
        Ok(iterator
            .filter(|(key, _)| after.is_none_or(|cursor| key.as_slice() > cursor))
            .take(limit)
            .collect())
    }

    pub(crate) fn raw_scan_prefix_for_collection(
        &self,
        collection: &str,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let tx = self.database_tx_for_collection(collection, TransactionMode::ReadOnly)?;
        let iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;
        Ok(iterator.collect())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn raw_scan_prefix_named(
        &self,
        family: &str,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let tx = self.transaction_by_name(family, TransactionMode::ReadOnly)?;
        let iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        let mut values = Vec::new();
        for (key, value) in iterator {
            values.push((key, value));
        }
        Ok(values)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn clear_temp_family(&self) -> Result<usize, CassieError> {
        let mut tx = self.temp_tx(TransactionMode::ReadWrite)?;
        let iterator = tx.scan(&Query::new()).map_err(CassieError::from)?;
        let mut keys = Vec::new();
        for (raw_key, _) in iterator {
            keys.push(raw_key);
        }

        if keys.is_empty() {
            return Ok(0);
        }

        let deleted = keys.len();
        for key in keys {
            tx.delete(key).map_err(CassieError::from)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(deleted)
    }
}
