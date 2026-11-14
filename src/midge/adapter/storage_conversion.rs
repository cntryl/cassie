//! Atomic conversion of a table between row and column authority.
//!
//! One database-family Midge transaction rewrites every row into the target
//! layout, removes the source layout and updates `CollectionMeta`. Inside an
//! explicit SQL transaction the adapter transaction stages into the session's
//! storage overlay, so the rewrite commits or rolls back with that transaction.
use super::column_store::validation::{decode_deleted_marker, decode_marker_parts};
use super::{
    collect_scan, decode_row, encode_row, key_encoding, CassieError, CollectionStorageMode, Midge,
    Query, RowSchema,
};
use std::collections::HashSet;

/// Budget callback: invoked with the retained byte estimate of each rewritten row.
pub(crate) type RewriteCharge<'a> = &'a mut dyn FnMut(usize) -> Result<(), CassieError>;

/// Number of rows a conversion rewrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConversionReport {
    pub rows: usize,
}

impl Midge {
    /// Converts `collection` to `target` authority in one transaction.
    ///
    /// Returns `None` without writing when the table already has that
    /// authority. Derived indexes (scalar, full-text, vector, time-series,
    /// graph) are keyed by field values, which conversion preserves, so they
    /// stay valid inside the same transaction. A column accelerator
    /// (`USING column`) is a derived layout the caller must reject first.
    ///
    /// # Errors
    ///
    /// Returns an error, with nothing written, when the table is missing, a
    /// value cannot be re-encoded, `charge` refuses a row, or storage fails.
    pub(crate) fn convert_collection_storage_mode(
        &self,
        collection: &str,
        target: CollectionStorageMode,
        charge: RewriteCharge<'_>,
    ) -> Result<Option<ConversionReport>, CassieError> {
        if !matches!(
            target,
            CollectionStorageMode::RowStore | CollectionStorageMode::ColumnStore
        ) {
            return Err(CassieError::Unsupported(format!(
                "cannot convert a table to storage mode '{}'",
                target.as_str()
            )));
        }
        let name = self.canonical_collection_name(collection);
        let gate = self.collection_write_gate(&name);
        let _guard = gate.lock();
        if target.uses_column_store_storage() {
            self.require_columnar_storage_layout()?;
        }
        let mut tx = self.begin_data_rw_tx_for(&name)?;
        let mut metadata = Self::load_collection_metadata_from_tx(&tx, &name)?
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
        if metadata.storage_mode.uses_column_store_storage() == target.uses_column_store_storage() {
            tx.rollback().map_err(CassieError::from)?;
            return Ok(None);
        }
        let mut row_schema = Self::load_row_schema_from_tx(&tx, &name)?
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;

        let rows = if target.uses_column_store_storage() {
            Self::rewrite_rows_to_columns(&mut tx, &name, &row_schema, charge)?
        } else {
            let rows = self.rewrite_columns_to_rows(&mut tx, &name, &mut row_schema, charge)?;
            Self::save_row_schema_to_tx(&mut tx, &name, &row_schema)?;
            rows
        };

        metadata.storage_mode = target;
        Self::save_collection_metadata_to_tx(&mut tx, &metadata)?;
        if let Err(error) = super::check_document_write_storage_failure_injection() {
            tx.rollback().map_err(CassieError::from)?;
            return Err(error);
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(Some(ConversionReport { rows }))
    }

    /// Rewrites a table's persisted storage version. Exists so tests can
    /// prove that hydration refuses a format this binary does not understand.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is missing or storage fails.
    #[doc(hidden)]
    pub fn set_collection_storage_version_for_diagnostics(
        &self,
        collection: &str,
        storage_version: u16,
    ) -> Result<(), CassieError> {
        let name = self.canonical_collection_name(collection);
        let mut tx = self.begin_data_rw_tx_for(&name)?;
        let mut metadata = Self::load_collection_metadata_from_tx(&tx, &name)?
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
        metadata.storage_version = storage_version;
        Self::save_collection_metadata_to_tx(&mut tx, &metadata)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    fn rewrite_rows_to_columns(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
        charge: RewriteCharge<'_>,
    ) -> Result<usize, CassieError> {
        let mut seen = HashSet::new();
        let mut converted = 0usize;
        for prefix in [
            Self::row_prefix(row_schema.relation_id),
            Self::doc_prefix(collection),
        ] {
            let entries = collect_scan(
                tx.scan(&Query::new().prefix(prefix.clone().into()))
                    .map_err(CassieError::from)?,
            )?;
            for (key, raw) in entries {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) else {
                    continue;
                };
                if id.is_empty() {
                    continue;
                }
                // A row key shadows a legacy document key with the same id.
                let first = seen.insert(id.clone());
                if first {
                    charge(retained_bytes(&key, &raw, &id))?;
                    let payload = decode_row(row_schema, &raw)?;
                    Self::write_column_store_document_to_tx(
                        tx, collection, &id, &payload, row_schema,
                    )?;
                    converted += 1;
                }
                tx.delete(key).map_err(CassieError::from)?;
            }
        }
        Ok(converted)
    }

    fn rewrite_columns_to_rows(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &mut RowSchema,
        charge: RewriteCharge<'_>,
    ) -> Result<usize, CassieError> {
        self.validate_column_store_field_markers(tx, collection, row_schema, || charge(0))?;
        let mut highest_revision = row_schema.column_revision_floor;
        let live_prefix = Self::column_store_row_prefix(row_schema.relation_id);
        let live = collect_scan(
            tx.scan(&Query::new().prefix(live_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut converted = 0usize;
        for (key, raw) in live {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &live_prefix) else {
                continue;
            };
            charge(retained_bytes(&key, &raw, &id))?;
            highest_revision = highest_revision.max(decode_marker_parts(collection, &id, &raw)?.1);
            // The marker contains no payload. Reserve the compact fields before
            // decoding them into retained JSON and the target row blob.
            for field in row_schema.fields.iter().filter(|field| !field.retired) {
                let field_key =
                    Self::column_store_field_key(row_schema.relation_id, field.field_id, &id);
                if let Some(value) = tx.get(&field_key).map_err(CassieError::from)? {
                    let decode_factor =
                        super::streaming_scans::decode_expansion_factor(&field.data_type);
                    // Keep the controlled-reader decode envelope and also retain
                    // compact input plus the target row encoding until commit.
                    let field_bytes = value
                        .len()
                        .checked_mul(decode_factor.saturating_add(2))
                        .and_then(|bytes| bytes.checked_add(field_key.len()))
                        .and_then(|bytes| bytes.checked_add(id.len()))
                        .ok_or_else(|| {
                            CassieError::ResourceLimit(
                                "column conversion field accounting overflow".to_owned(),
                            )
                        })?;
                    charge(field_bytes)?;
                }
            }
            let payload =
                Self::load_column_store_document_from_tx(tx, collection, &id, row_schema)?
                    .ok_or_else(|| {
                        CassieError::Execution(format!(
                            "column-store row '{id}' of '{collection}' vanished during conversion"
                        ))
                    })?;
            let blob = encode_row(row_schema, &payload)?;
            tx.put(Self::row_key(row_schema.relation_id, &id), blob, None)
                .map_err(CassieError::from)?;
            converted += 1;
        }
        // Remove every column-family key (live markers, fields, tombstones) so
        // the row table has no residue in the column layout.
        let family_prefix = Self::column_store_collection_prefix(row_schema.relation_id);
        let residue = collect_scan(
            tx.scan(&Query::new().prefix(family_prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let deleted_prefix = Self::column_store_deleted_prefix(row_schema.relation_id);
        for (key, raw) in residue {
            if key.starts_with(&deleted_prefix) {
                highest_revision = highest_revision.max(decode_deleted_marker(&raw));
            }
            tx.delete(key).map_err(CassieError::from)?;
        }
        row_schema.column_revision_floor = highest_revision;
        Ok(converted)
    }
}

/// Conservative retained-memory estimate for one decoded and re-encoded row.
fn retained_bytes(key: &[u8], raw: &[u8], id: &str) -> usize {
    key.len()
        .saturating_add(raw.len().saturating_mul(4))
        .saturating_add(id.len())
        .saturating_add(256)
}
