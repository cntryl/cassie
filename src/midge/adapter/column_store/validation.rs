//! Corruption rules for column-authoritative rows.
//!
//! A live marker records the `RowSchema` version the row was written at. A
//! non-retired field introduced at or before that version must have a field
//! key; a field introduced later resolves from its default metadata. Any
//! violation is data corruption (`XX001`) that names the relation and row id.

use super::{collect_scan, key_encoding, CassieError, Midge, Query};
use crate::midge::row_blob::{RowFieldMeta, RowSchema};

const VERSION_LEN: usize = 4;
const REVISION_LEN: usize = 8;
const MARKER_LEN: usize = VERSION_LEN + REVISION_LEN;

/// The SQLSTATE every column-store corruption reports.
pub(crate) const CORRUPTION_SQLSTATE: &str = "XX001";

/// Builds the `XX001` error for a corrupt column-store row.
pub(crate) fn corrupt_row(relation: &str, id: &str, detail: &str) -> CassieError {
    CassieError::from(crate::app::DiagnosticError::new(
        CORRUPTION_SQLSTATE,
        format!("column table '{relation}' row '{id}' is corrupt: {detail}"),
    ))
}

/// Encodes the live-marker value: the row's schema version (`u32`) followed by
/// its write revision (`u64`), both big-endian.
///
/// The revision changes on every write, so a REPEATABLE READ fence over the
/// marker key observes a concurrent update that leaves the schema unchanged.
pub(crate) fn encode_marker(schema_version: u32, revision: u64) -> Vec<u8> {
    let mut marker = Vec::with_capacity(MARKER_LEN);
    marker.extend_from_slice(&schema_version.to_be_bytes());
    marker.extend_from_slice(&revision.to_be_bytes());
    marker
}

/// Decodes a live-marker value into the schema version the row was written at.
///
/// # Errors
///
/// Returns `XX001` when the marker is not a `u32` version and a `u64` revision.
pub(crate) fn decode_marker(relation: &str, id: &str, raw: &[u8]) -> Result<u32, CassieError> {
    decode_marker_parts(relation, id, raw).map(|(version, _)| version)
}

/// Decodes a live marker into its schema version and write revision.
///
/// # Errors
///
/// Returns `XX001` when the marker has the wrong length.
pub(crate) fn decode_marker_parts(
    relation: &str,
    id: &str,
    raw: &[u8],
) -> Result<(u32, u64), CassieError> {
    let wrong_length = || {
        corrupt_row(
            relation,
            id,
            &format!(
                "live marker holds {} bytes instead of {MARKER_LEN}",
                raw.len()
            ),
        )
    };
    if raw.len() != MARKER_LEN {
        return Err(wrong_length());
    }
    let (version, revision) = raw.split_at(VERSION_LEN);
    let version = u32::from_be_bytes(version.try_into().map_err(|_| wrong_length())?);
    let revision = u64::from_be_bytes(revision.try_into().map_err(|_| wrong_length())?);
    Ok((version, revision))
}

/// Encodes the deleted-marker value: the revision of the delete, big-endian.
pub(crate) fn encode_deleted_marker(revision: u64) -> Vec<u8> {
    revision.to_be_bytes().to_vec()
}

/// Decodes a deleted-marker value; a malformed one restarts the revision.
pub(crate) fn decode_deleted_marker(raw: &[u8]) -> u64 {
    <[u8; REVISION_LEN]>::try_from(raw).map_or(0, u64::from_be_bytes)
}

/// Whether a row written at `marker_version` must store a value for `field`.
pub(crate) fn field_is_required(field: &RowFieldMeta, marker_version: u32) -> bool {
    !field.retired && field.added_version <= marker_version
}

/// The `XX001` error for a required field key that is absent.
pub(crate) fn missing_field(relation: &str, id: &str, field: &RowFieldMeta) -> CassieError {
    corrupt_row(
        relation,
        id,
        &format!(
            "live marker has no value for field '{}' (field id {})",
            field.name, field.field_id
        ),
    )
}

/// Decodes one stored field value, mapping a decode or type failure to `XX001`.
///
/// Resource, cancellation and diagnostic errors pass through unchanged.
///
/// # Errors
///
/// Returns `XX001` when the stored bytes do not decode as the declared type.
pub(crate) fn decode_field(
    relation: &str,
    id: &str,
    field: &RowFieldMeta,
    raw: &[u8],
) -> Result<Option<serde_json::Value>, CassieError> {
    match crate::midge::row_blob::decode_compact_field_value(&field.data_type, raw) {
        Err(CassieError::Parse(detail)) => Err(corrupt_row(
            relation,
            id,
            &format!("field '{}' does not decode: {detail}", field.name),
        )),
        other => other,
    }
}

impl Midge {
    /// Reads one field of a column row and validates it against the marker.
    ///
    /// Returns `None` when the value is SQL NULL for a JSON column, or when
    /// the field postdates the row and resolves from default metadata.
    ///
    /// # Errors
    ///
    /// Returns `XX001` when a required field key is absent or does not decode.
    pub(crate) fn read_column_store_field(
        tx: &crate::midge::adapter::AdapterTransaction,
        relation: &str,
        id: &str,
        row_schema: &RowSchema,
        marker_version: u32,
        field: &RowFieldMeta,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        let raw = tx
            .get(&Self::column_store_field_key(
                row_schema.relation_id,
                field.field_id,
                id,
            ))
            .map_err(CassieError::from)?;
        match raw {
            Some(raw) => decode_field(relation, id, field, &raw),
            None if field_is_required(field, marker_version) => {
                Err(missing_field(relation, id, field))
            }
            None => Ok(None),
        }
    }

    /// Fails with `XX001` when any non-retired field key exists for a row
    /// that has no live marker.
    ///
    /// # Errors
    ///
    /// Returns `XX001` for an orphan field key and storage errors otherwise.
    pub(crate) fn reject_orphan_column_store_fields(
        tx: &crate::midge::adapter::AdapterTransaction,
        relation: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let present = tx
                .get(&Self::column_store_field_key(
                    row_schema.relation_id,
                    field.field_id,
                    id,
                ))
                .map_err(CassieError::from)?
                .is_some();
            if present {
                return Err(corrupt_row(
                    relation,
                    id,
                    &format!(
                        "field '{}' has a value but the row has no live marker",
                        field.name
                    ),
                ));
            }
        }
        Ok(())
    }
}

impl Midge {
    /// Checks every row of a column table and returns one message per
    /// violation: a live marker missing a required field key, a field key
    /// without a live marker, a value that fails to decode, a malformed
    /// marker, or a row blob or legacy document key the table must not own.
    ///
    /// # Errors
    ///
    /// Returns storage errors; corruption is reported as violations.
    pub(crate) fn verify_column_store_rows(
        &self,
        collection: &str,
    ) -> Result<Vec<String>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let row_schema = self.row_schema(&collection)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let mut violations = Vec::new();
        let live = Self::verify_column_markers(&tx, &collection, &row_schema, &mut violations)?;
        Self::verify_column_values(&tx, &collection, &row_schema, &live, &mut violations)?;
        Self::verify_column_orphans(&tx, &collection, &row_schema, &live, &mut violations)?;
        Self::verify_no_row_store_keys(&tx, &collection, &row_schema, &mut violations)?;
        Ok(violations)
    }

    /// Collects the live rows and the schema version each was written at.
    fn verify_column_markers(
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
        violations: &mut Vec<String>,
    ) -> Result<std::collections::BTreeMap<String, u32>, CassieError> {
        let marker_prefix = Self::column_store_row_prefix(row_schema.relation_id);
        let mut live = std::collections::BTreeMap::new();
        for (key, marker) in collect_scan(
            tx.scan(&Query::new().prefix(marker_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )? {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &marker_prefix) else {
                continue;
            };
            match decode_marker(collection, &id, &marker) {
                Ok(version) => {
                    live.insert(id, version);
                }
                Err(error) => violations.push(error.to_string()),
            }
        }
        Ok(live)
    }

    /// Reads every field of every live row, recording each `XX001` failure.
    fn verify_column_values(
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
        live: &std::collections::BTreeMap<String, u32>,
        violations: &mut Vec<String>,
    ) -> Result<(), CassieError> {
        for (id, marker_version) in live {
            for field in row_schema.fields.iter().filter(|field| !field.retired) {
                match Self::read_column_store_field(
                    tx,
                    collection,
                    id,
                    row_schema,
                    *marker_version,
                    field,
                ) {
                    Ok(_) => {}
                    Err(error) if error.sql_state() == CORRUPTION_SQLSTATE => {
                        violations.push(error.to_string());
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    /// Records every field key whose row has no live marker.
    fn verify_column_orphans(
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
        live: &std::collections::BTreeMap<String, u32>,
        violations: &mut Vec<String>,
    ) -> Result<(), CassieError> {
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let field_prefix =
                Self::column_store_field_prefix(row_schema.relation_id, field.field_id);
            for (key, _) in collect_scan(
                tx.scan(&Query::new().prefix(field_prefix.clone().into()))
                    .map_err(CassieError::from)?,
            )? {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &field_prefix) else {
                    continue;
                };
                if !live.contains_key(&id) {
                    let detail = format!(
                        "field '{}' has a value but the row has no live marker",
                        field.name
                    );
                    violations.push(corrupt_row(collection, &id, &detail).to_string());
                }
            }
        }
        Ok(())
    }

    /// Records a row blob or legacy document key, which column tables never own.
    fn verify_no_row_store_keys(
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
        violations: &mut Vec<String>,
    ) -> Result<(), CassieError> {
        for (kind, prefix) in [
            ("row blob", Self::row_prefix(row_schema.relation_id)),
            ("legacy document", Self::doc_prefix(collection)),
        ] {
            let found = collect_scan(
                tx.scan(&Query::new().prefix(prefix.into()).limit(1))
                    .map_err(CassieError::from)?,
            )?;
            if !found.is_empty() {
                violations.push(format!(
                    "column table '{collection}' owns a {kind} key, which column tables must not hold"
                ));
            }
        }
        Ok(())
    }
}

impl Midge {
    /// Key prefixes a column table must never own: its row-blob family and its
    /// legacy document family. Test and diagnostic support for the
    /// no-row-blob invariant.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection is unknown.
    #[doc(hidden)]
    pub fn row_store_prefixes_for_diagnostics(
        &self,
        collection: &str,
    ) -> Result<Vec<Vec<u8>>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let row_schema = self.row_schema(&collection)?;
        Ok(vec![
            Self::row_prefix(row_schema.relation_id),
            Self::doc_prefix(&collection),
        ])
    }

    /// Runs the column-store verifier and returns its violation messages.
    ///
    /// # Errors
    ///
    /// Returns storage errors; corruption is reported as messages.
    #[doc(hidden)]
    pub fn verify_column_store_for_diagnostics(
        &self,
        collection: &str,
    ) -> Result<Vec<String>, CassieError> {
        self.verify_column_store_rows(collection)
    }
}
