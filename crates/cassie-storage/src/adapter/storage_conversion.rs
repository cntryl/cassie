//! Atomic conversion of a table between row and column authority.
//!
//! One database-family Midge transaction rewrites every row into the target
//! layout, removes the source layout and updates `CollectionMeta`. Inside an
//! explicit SQL transaction the adapter transaction stages into the session's
//! storage overlay, so the rewrite commits or rolls back with that transaction.
use super::column_store::validation::{decode_deleted_marker, decode_marker_parts};
use super::{
    decode_row, encode_row, key_encoding, CassieError, CollectionStorageMode, Midge, Query,
    RowSchema,
};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

fn collect_conversion_scan(
    mut scan: cntryl_midge::ScanIterator<'_>,
    charge: RewriteCharge<'_>,
) -> Result<Vec<super::RawStorageEntry>, CassieError> {
    let mut entries = Vec::new();
    loop {
        charge(0)?;
        let Some(entry) = scan.next() else {
            return Ok(entries);
        };
        let (key, value) = entry.map_err(CassieError::from)?;
        #[cfg(test)]
        tests::observe_yield(&key);
        let capacity = if entries.len() == entries.capacity() {
            entries
                .len()
                .checked_add(1)
                .and_then(|length| length.max(4).checked_next_power_of_two())
                .ok_or_else(scan_accounting_overflow)?
        } else {
            0
        };
        // Retain the whole next allocation's admission alongside earlier
        // capacity charges, including any old/new allocation overlap.
        let retained = capacity
            .checked_mul(std::mem::size_of::<super::RawStorageEntry>())
            .and_then(|bytes| bytes.checked_add(key.len()))
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or_else(scan_accounting_overflow)?;
        charge(retained)?;
        if capacity != 0 {
            entries
                .try_reserve_exact(capacity - entries.len())
                .map_err(|error| {
                    CassieError::ResourceLimit(format!(
                        "unable to retain conversion source entries: {error}"
                    ))
                })?;
        }
        #[cfg(test)]
        tests::observe_copy(&key);
        entries.push((key.to_vec(), value.to_vec()));
    }
}

fn scan_accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("column conversion scan accounting overflow".to_owned())
}

/// Admission callback for retained source buffers and rewritten rows.
/// A zero-byte charge checks interruption without adding retained state.
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
        charge(0)?;
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
        tx: &mut crate::adapter::AdapterTransaction,
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
            charge(0)?;
            let entries = collect_conversion_scan(
                tx.scan(&Query::new().prefix(prefix.clone().into()))
                    .map_err(CassieError::from)?,
                charge,
            )?;
            for (key, raw) in entries {
                charge(retained_bytes(&key, &raw, key.len()))?;
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) else {
                    continue;
                };
                if id.is_empty() {
                    continue;
                }
                // A row key shadows a legacy document key with the same id.
                let first = seen.insert(id.clone());
                if first {
                    let payload = crate::adapter::decode_row_with_hooks(row_schema, &raw, tx.value_hooks())?;
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
        tx: &mut crate::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &mut RowSchema,
        charge: RewriteCharge<'_>,
    ) -> Result<usize, CassieError> {
        self.validate_column_store_field_markers(tx, collection, row_schema, || charge(0))?;
        let mut highest_revision = row_schema.column_revision_floor;
        let live_prefix = Self::column_store_row_prefix(row_schema.relation_id);
        charge(0)?;
        let live = collect_conversion_scan(
            tx.scan(&Query::new().prefix(live_prefix.clone().into()))
                .map_err(CassieError::from)?,
            charge,
        )?;
        let mut converted = 0usize;
        for (key, raw) in live {
            charge(retained_bytes(&key, &raw, key.len()))?;
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &live_prefix) else {
                continue;
            };
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
                    let field_bytes = super::column_store::column_payload_logical_len(&value)
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
        charge(0)?;
        let residue = collect_conversion_scan(
            tx.scan(&Query::new().prefix(family_prefix.into()))
                .map_err(CassieError::from)?,
            charge,
        )?;
        let deleted_prefix = Self::column_store_deleted_prefix(row_schema.relation_id);
        for (key, raw) in residue {
            charge(0)?;
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
fn retained_bytes(key: &[u8], raw: &[u8], id_bytes: usize) -> usize {
    key.len()
        .saturating_add(raw.len().saturating_mul(4))
        .saturating_add(id_bytes)
        .saturating_add(256)
}
