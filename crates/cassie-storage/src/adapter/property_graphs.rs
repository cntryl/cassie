//! Persistence of mapped property graph definitions (#1255).
//!
//! A definition lives in the Data family of its database, so it commits
//! atomically with any other change of the same transaction. Nothing derived
//! is stored: graph reads go to the mapped tables, through their ordinary indexes
//! where present.

use super::{key_encoding, CassieError, Midge};
use crate::catalog::PropertyGraphMeta;

impl Midge {
    /// The durable identity of `canonical_name`'s database, allocated on first
    /// use and never changed afterwards.
    fn ensure_database_identity(
        tx: &mut crate::midge::adapter::AdapterTransaction,
    ) -> Result<String, CassieError> {
        let key = key_encoding::database_identity_key();
        if let Some(raw) = tx.get(&key).map_err(CassieError::from)? {
            return String::from_utf8(raw.to_vec()).map_err(|error| {
                CassieError::Storage(format!("invalid database identity: {error}"))
            });
        }
        let identity = uuid::Uuid::new_v4().to_string();
        tx.put(key, identity.clone().into_bytes(), None)
            .map_err(CassieError::from)?;
        Ok(identity)
    }

    /// Stores a new definition. Its object id and the owning database identity
    /// are assigned here; the stored definition is returned.
    ///
    /// # Errors
    ///
    /// Returns an error when storage fails.
    pub fn put_property_graph(
        &self,
        metadata: &PropertyGraphMeta,
    ) -> Result<PropertyGraphMeta, CassieError> {
        let mut metadata = metadata.clone();
        if metadata.object_id == 0 {
            metadata.object_id = self.allocate_object_id()?;
        }
        let mut tx = self.begin_data_rw_tx_for(&metadata.name)?;
        metadata.database_id = Self::ensure_database_identity(&mut tx)?;
        Self::write_property_graph(&mut tx, &metadata)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(metadata)
    }

    fn write_property_graph(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        metadata: &PropertyGraphMeta,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            key_encoding::property_graph_key(&metadata.name),
            value,
            None,
        )
        .map_err(CassieError::from)
    }

    /// Every stored definition, across all databases.
    ///
    /// # Errors
    ///
    /// Returns an error when storage fails or a definition cannot be decoded.
    pub fn list_property_graphs(&self) -> Result<Vec<PropertyGraphMeta>, CassieError> {
        self.scan_database_families(&key_encoding::property_graph_prefix())?
            .into_iter()
            .map(|(_key, raw)| {
                serde_json::from_slice(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid property graph definition: {error}"))
                })
            })
            .collect()
    }

    /// Removes a definition. Mapped tables and their rows are not touched.
    ///
    /// # Errors
    ///
    /// Returns an error when storage fails.
    pub fn delete_property_graph(&self, canonical_name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(canonical_name)?;
        tx.delete(key_encoding::property_graph_key(canonical_name))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// Moves a definition to `renamed.name` in one transaction, keeping its
    /// identities.
    ///
    /// # Errors
    ///
    /// Returns an error when storage fails.
    pub fn rename_property_graph(
        &self,
        previous_name: &str,
        renamed: &PropertyGraphMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(previous_name)?;
        tx.delete(key_encoding::property_graph_key(previous_name))
            .map_err(CassieError::from)?;
        Self::write_property_graph(&mut tx, renamed)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}
