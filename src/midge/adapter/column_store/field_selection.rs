//! Selected-field reads of column-authoritative rows (#1356).
//!
//! A column row is one live marker plus one key per field, so a query that
//! selects two fields needs the marker and two field keys. `ColumnFieldSelection`
//! resolves the selected fields once per scan; the reads below then fetch and
//! decode only those fields. Three counters make the cost observable and
//! separate from the ownership validation counted by
//! `column_validation_reads_for_diagnostics`.

use crate::midge::row_blob::{RowFieldMeta, RowSchema};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// The non-retired fields a read fetches, as indexes into the row schema in
/// schema order. `None` projection selects every non-retired field.
#[derive(Clone, Debug, Default)]
pub(crate) struct ColumnFieldSelection {
    indices: Vec<usize>,
}

impl ColumnFieldSelection {
    pub(crate) fn new(row_schema: &RowSchema, projection: Option<&HashSet<String>>) -> Self {
        let indices = row_schema
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| !field.retired)
            .filter(|(_, field)| {
                projection.is_none_or(|selected| {
                    selected.contains(
                        &crate::sql::ColumnIdentifierPath::from_field_name(&field.name)
                            .lookup_key(),
                    )
                })
            })
            .map(|(index, _)| index)
            .collect();
        Self { indices }
    }

    pub(crate) fn fields<'a>(
        &'a self,
        row_schema: &'a RowSchema,
    ) -> impl Iterator<Item = &'a RowFieldMeta> + 'a {
        self.indices
            .iter()
            .filter_map(|index| row_schema.fields.get(*index))
    }
}

/// Process-lifetime counters of field payload work on column tables.
#[derive(Debug, Default)]
pub(crate) struct ColumnFieldReadCounters {
    payload_reads: AtomicU64,
    payload_bytes: AtomicU64,
    decodes: AtomicU64,
    retained_bytes: AtomicU64,
}

impl ColumnFieldReadCounters {
    /// A stored field payload was fetched from Midge.
    pub(crate) fn record_payload(&self, bytes: usize) {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        self.payload_bytes
            .fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    /// A fetched payload was decoded into a value.
    pub(crate) fn record_decode(&self) {
        self.decodes.fetch_add(1, Ordering::Relaxed);
    }

    /// Bytes admitted for a decoded selected value that the result retains.
    pub(crate) fn record_retained(&self, bytes: usize) {
        self.retained_bytes
            .fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> ColumnFieldReadSnapshot {
        ColumnFieldReadSnapshot {
            payload_reads: self.payload_reads.load(Ordering::Relaxed),
            payload_bytes: self.payload_bytes.load(Ordering::Relaxed),
            decodes: self.decodes.load(Ordering::Relaxed),
            retained_bytes: self.retained_bytes.load(Ordering::Relaxed),
        }
    }
}

/// The live row a selected-field read targets.
#[derive(Clone, Copy)]
pub(crate) struct ColumnRowRef<'a> {
    pub relation: &'a str,
    pub id: &'a str,
    pub row_schema: &'a RowSchema,
    pub marker_version: u32,
}

impl crate::midge::adapter::Midge {
    /// Reads and validates one selected field of a live column row, counting
    /// the fetched payload and its decode. Behaves exactly like
    /// `read_column_store_field` otherwise.
    ///
    /// # Errors
    ///
    /// Returns `XX001` when a required field key is absent or does not decode.
    pub(crate) fn read_counted_column_store_field(
        &self,
        tx: &crate::midge::adapter::AdapterTransaction,
        row: ColumnRowRef<'_>,
        field: &RowFieldMeta,
    ) -> Result<Option<serde_json::Value>, crate::app::CassieError> {
        let raw = tx
            .get(&Self::column_store_field_key(
                row.row_schema.relation_id,
                field.field_id,
                row.id,
            ))
            .map_err(crate::app::CassieError::from)?;
        let counters = self.column_field_reads();
        match raw {
            Some(raw) => {
                counters.record_payload(raw.len());
                counters.record_decode();
                super::validation::decode_field(row.relation, row.id, field, &raw)
            }
            None if super::validation::field_is_required(field, row.marker_version) => Err(
                super::validation::missing_field(row.relation, row.id, field),
            ),
            None => Ok(None),
        }
    }
}

/// A point-in-time copy of the field read counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnFieldReadSnapshot {
    /// Field payloads fetched from Midge.
    pub payload_reads: u64,
    /// Raw bytes of the fetched field payloads.
    pub payload_bytes: u64,
    /// Fetched payloads decoded into values.
    pub decodes: u64,
    /// Bytes admitted for decoded values retained by the result.
    pub retained_bytes: u64,
}

impl ColumnFieldReadSnapshot {
    /// The work done since `earlier`.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            payload_reads: self.payload_reads.saturating_sub(earlier.payload_reads),
            payload_bytes: self.payload_bytes.saturating_sub(earlier.payload_bytes),
            decodes: self.decodes.saturating_sub(earlier.decodes),
            retained_bytes: self.retained_bytes.saturating_sub(earlier.retained_bytes),
        }
    }
}
