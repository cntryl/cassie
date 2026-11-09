//! Unlink collection names separately from deferred physical relation cleanup.

use super::{
    check_collection_drop_failure_point, collect_scan, key_encoding, CassieError, Midge, Query,
    RetentionPolicyMeta,
};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn drop_collection(&self, name: &str) -> Result<(), CassieError> {
        let name_storage = self.canonical_collection_name(name);
        let name = name_storage.as_str();
        let write_gate = self.collection_write_gate(name);
        let _write_guard = write_gate.lock();
        let relation_id = self.unlink_collection_metadata(name)?;
        check_collection_drop_failure_point()?;
        self.delete_collection_data(name, relation_id)
    }

    /// Only the private DDL overlay loses the name; published readers retain it.
    pub(in crate::midge::adapter) fn unlink_transactional_collection_name(
        &self,
        name: &str,
    ) -> Result<(), CassieError> {
        let name = self.canonical_collection_name(name);
        let write_gate = self.collection_write_gate(&name);
        let _write_guard = write_gate.lock();
        self.unlink_collection_metadata(&name)?;
        self.delete_collection_data_parts(&name, None, true)
    }

    fn unlink_collection_metadata(&self, name: &str) -> Result<u64, CassieError> {
        let mut schema_tx = self.begin_data_rw_tx_for(name)?;
        let schema_key = Self::collection_schema_key(name);
        if schema_tx
            .get(&schema_key)
            .map_err(CassieError::from)?
            .is_none()
        {
            return Err(CassieError::CollectionNotFound(name.to_string()));
        }
        let relation_id = Self::load_row_schema_from_tx(&schema_tx, name)?
            .ok_or_else(|| CassieError::Parse("missing row schema metadata".to_string()))?
            .relation_id;

        let vector_prefix = Self::vector_index_collection_prefix(name);
        let vector_indexes = collect_scan(
            schema_tx
                .scan(&Query::new().prefix(vector_prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let mut vector_keys = Vec::new();
        for (key, _value) in vector_indexes {
            vector_keys.push(key);
        }
        for key in vector_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }

        let index_prefix = Self::index_collection_prefix(name);
        let index_scan = collect_scan(
            schema_tx
                .scan(&Query::new().prefix(index_prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let mut index_keys = Vec::new();
        for (key, _) in index_scan {
            index_keys.push(key);
        }
        for key in index_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }
        let retention_scan = collect_scan(
            schema_tx
                .scan(&Query::new().prefix(Self::retention_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut retention_keys = Vec::new();
        for (key, value) in retention_scan {
            let Ok(policy) = serde_json::from_slice::<RetentionPolicyMeta>(&value) else {
                continue;
            };
            if policy.collection == name {
                retention_keys.push(key);
            }
        }
        for key in retention_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }
        Self::delete_collection_metadata_to_tx(&mut schema_tx, name)?;

        schema_tx
            .delete(Self::constraints_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::row_schema_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::projection_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::cardinality_key(name))
            .map_err(CassieError::from)?;

        let mut collections = Self::load_collections(&schema_tx)?;
        collections.retain(|entry| entry != name);
        Self::save_collections(&mut schema_tx, &collections)?;
        schema_tx.delete(schema_key).map_err(CassieError::from)?;
        schema_tx
            .commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(relation_id)
    }

    pub(in crate::midge::adapter) fn delete_collection_data(
        &self,
        name: &str,
        relation_id: u64,
    ) -> Result<(), CassieError> {
        self.delete_collection_data_parts(name, Some(relation_id), true)
    }

    pub(in crate::midge::adapter) fn delete_retired_collection_data(
        &self,
        name: &str,
        relation_id: u64,
    ) -> Result<(), CassieError> {
        self.delete_collection_data_parts(name, Some(relation_id), false)
    }

    fn delete_collection_data_parts(
        &self,
        name: &str,
        relation_id: Option<u64>,
        remove_name_keys: bool,
    ) -> Result<(), CassieError> {
        let canonical_name = self.canonical_collection_name(name);
        let mut data_tx = self.begin_data_rw_tx_for(name)?;
        let mut document_keys = Vec::new();
        let mut data_prefixes = Vec::new();
        if let Some(relation_id) = relation_id {
            data_prefixes.extend([
                Self::row_prefix(relation_id),
                Self::scalar_index_collection_prefix(relation_id),
                Self::time_series_index_collection_prefix(relation_id),
                Self::fulltext_index_collection_prefix(relation_id),
                Self::normalized_vector_collection_prefix(relation_id),
                Self::vector_index_state_prefix(relation_id),
                Self::column_batch_collection_prefix(relation_id),
                Self::column_store_collection_prefix(relation_id),
            ]);
        }
        if remove_name_keys {
            data_prefixes.extend([
                Self::doc_prefix(name),
                key_encoding::unique_constraint_reservation_prefix(name),
                key_encoding::unique_index_reservation_prefix(name),
                Self::row_hash_prefix(name),
                Self::range_hash_prefix(name),
            ]);
            data_prefixes.extend(
                self.list_graphs()?
                    .into_iter()
                    .filter(|graph| {
                        self.canonical_collection_name(&graph.node_collection)
                            .eq_ignore_ascii_case(&canonical_name)
                            || self
                                .canonical_collection_name(&graph.edge_collection)
                                .eq_ignore_ascii_case(&canonical_name)
                    })
                    .map(|graph| Self::graph_adjacency_prefix(graph.storage_id)),
            );
        }
        for data_prefix in data_prefixes {
            let documents = collect_scan(
                data_tx
                    .scan(&Query::new().prefix(data_prefix.into()))
                    .map_err(CassieError::from)?,
            )?;
            for (key, _value) in documents {
                document_keys.push(key);
            }
        }

        for key in document_keys {
            data_tx.delete(key).map_err(CassieError::from)?;
        }
        if !remove_name_keys {
            return data_tx
                .commit(self.write_options_sync())
                .map_err(CassieError::from);
        }
        let debt_entries = collect_scan(
            data_tx
                .scan(&Query::new().prefix(Self::maintenance_debt_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        for (key, value) in debt_entries {
            let collection_matches = serde_json::from_slice::<serde_json::Value>(&value)
                .ok()
                .and_then(|debt| {
                    debt.get("collection")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .is_some_and(|collection| collection == name);
            if collection_matches {
                data_tx.delete(key).map_err(CassieError::from)?;
            }
        }
        data_tx
            .delete(Self::collection_generation_key(name))
            .map_err(CassieError::from)?;
        data_tx
            .delete(Self::root_hash_key(name))
            .map_err(CassieError::from)?;
        data_tx
            .commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}
