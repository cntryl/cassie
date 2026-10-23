use super::super::index_publication::PendingIndexPublication;
use super::super::{check_index_publication_failure_point, CassieError, IndexMeta, Midge};

impl Midge {
    /// Adopts a unique index and persists its constraint metadata in one data transaction.
    /// The caller must hold the collection's canonical write gate.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is missing, the destination exists, or storage fails.
    pub(crate) fn adopt_unique_index_and_constraints_with_held_collection_gate(
        &self,
        collection: &str,
        current_name: &str,
        next_name: &str,
        constraints: &[crate::catalog::FieldConstraint],
    ) -> Result<IndexMeta, CassieError> {
        crate::sql::definition_guard::constraints(constraints)?;
        let collection = self.canonical_collection_name(collection);
        let current_key = Midge::index_key(&collection, current_name);
        let next_key = Midge::index_key(&collection, next_name);
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        let raw = tx
            .get(&current_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Index,
                name: current_name.to_string(),
            })?;
        if current_key != next_key && tx.get(&next_key).map_err(CassieError::from)?.is_some() {
            return Err(CassieError::CatalogObjectAlreadyExists {
                kind: crate::app::CatalogObjectKind::Index,
                name: next_name.to_string(),
            });
        }
        let mut metadata: IndexMeta = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))?;
        metadata.set_catalog_oid_if_missing(crate::catalog::object_identity::stable_index_oid(
            &metadata.name,
        ));
        if metadata.unique {
            let reservation_name = metadata.name.clone();
            metadata.set_unique_reservation_name_if_missing(&reservation_name);
        }
        metadata.name = next_name.to_string();
        if current_key != next_key {
            tx.delete(current_key).map_err(CassieError::from)?;
        }
        tx.put(
            next_key,
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.put(
            Midge::constraints_key(&collection),
            serde_json::to_vec(constraints)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(metadata)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_index(&self, metadata: &IndexMeta) -> Result<(), CassieError> {
        self.prepare_index_for_publication(metadata)?;
        check_index_publication_failure_point()?;
        self.replay_pending_index_publications()
    }

    /// Updates durable index metadata and publishes the catalog update under one collection gate.
    ///
    /// The update runs against the latest metadata while holding the collection write gate.
    /// After commit, `publish` runs before that gate is released. Relation and storage
    /// identities cannot change through this operation.
    ///
    /// # Errors
    ///
    /// Returns an error when the index no longer exists, its identity changes, or storage fails.
    pub(crate) fn update_index_metadata(
        &self,
        collection: &str,
        name: &str,
        update: impl FnOnce(&mut IndexMeta) -> Result<(), CassieError>,
        publish: impl FnOnce(IndexMeta),
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        let key = Self::index_key(&collection, name);
        let raw = tx.get(&key).map_err(CassieError::from)?;
        let mut metadata: IndexMeta = raw
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Index,
                name: name.to_string(),
            })
            .and_then(|raw| {
                serde_json::from_slice(&raw)
                    .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))
            })?;
        let identity = (
            metadata.kind.clone(),
            metadata.collection.clone(),
            metadata.name.clone(),
            metadata.relation_id(),
            metadata.storage_id(),
            metadata.access_method().to_string(),
        );
        update(&mut metadata)?;
        if metadata.kind != identity.0
            || metadata.collection != identity.1
            || metadata.name != identity.2
            || metadata.relation_id() != identity.3
            || metadata.storage_id() != identity.4
            || metadata.access_method() != identity.5
        {
            return Err(CassieError::Execution(
                "index metadata replacement cannot change index identity".to_string(),
            ));
        }
        tx.put(
            key,
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        publish(metadata);
        Ok(())
    }

    /// Publishes an index while the caller owns the index collection's canonical write gate.
    ///
    /// This does not acquire the referential gate or replay other pending index publications.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub(crate) fn put_index_with_held_collection_gate(
        &self,
        metadata: &IndexMeta,
    ) -> Result<(), CassieError> {
        let publication = self.prepare_index_for_publication(metadata)?;
        check_index_publication_failure_point()?;
        self.publish_prepared_index_locked(publication)
    }

    fn prepare_index_for_publication(
        &self,
        metadata: &IndexMeta,
    ) -> Result<PendingIndexPublication, CassieError> {
        let metadata = self.prepare_index_metadata(metadata)?;
        self.prepare_index_publication(&metadata)
    }

    pub(in crate::midge::adapter) fn prepare_index_metadata(
        &self,
        metadata: &IndexMeta,
    ) -> Result<IndexMeta, CassieError> {
        for expr in metadata.expressions.iter().chain(&metadata.predicate) {
            crate::sql::definition_guard::encoded_expression(expr)?;
        }
        let mut metadata = metadata.clone();
        metadata.collection = self.canonical_collection_name(&metadata.collection);
        let relation_id = self
            .collection_metadata(&metadata.collection)?
            .ok_or_else(|| CassieError::CollectionNotFound(metadata.collection.clone()))?
            .storage_id;
        let storage_id =
            if let Some(existing) = self.get_index(&metadata.collection, &metadata.name)? {
                existing.storage_id().ok_or_else(|| {
                    CassieError::Parse("index metadata is missing its storage id".to_string())
                })?
            } else {
                let mut tx = self.begin_schema_rw_tx()?;
                let id = Self::allocate_object_id_to_tx(&mut tx)?;
                tx.commit(self.write_options_sync())
                    .map_err(CassieError::from)?;
                id
            };
        metadata.set_storage_ids(relation_id, storage_id);
        Ok(metadata)
    }
}

#[cfg(test)]
#[path = "index_lifecycle_tests.rs"]
mod tests;
