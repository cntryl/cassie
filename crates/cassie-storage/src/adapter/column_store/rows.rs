//! Row-level reads, writes and scans of column-authoritative tables.

use super::super::{DocumentRef, Instant, MidgeScanTimings};
use super::field_selection::{ColumnFieldSelection, ColumnRowRef};
use super::validation::{
    decode_deleted_marker, decode_marker, decode_marker_parts, encode_deleted_marker, encode_marker,
};
use super::{
    collect_scan, key_encoding, CassieError, ColumnStoreScanRequest, Midge, Query, RowFilter,
    RowSchema,
};
use std::time::Duration;

impl Midge {
    /// Loads one live column row, or `None` when it has no live marker.
    ///
    /// # Errors
    ///
    /// Returns `XX001` when the row's field keys contradict its marker.
    pub(crate) fn load_column_store_document_from_tx(
        tx: & crate::adapter::AdapterTransaction,
        collection: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        let Some(marker) = tx
            .get(&Self::column_store_row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let marker_version = decode_marker(collection, id, &marker)?;

        let mut payload = serde_json::Map::new();
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let Some(value) = Self::read_column_store_field(
                tx,
                collection,
                id,
                row_schema,
                marker_version,
                field,
            )?
            else {
                continue;
            };
            payload.insert(field.name.clone(), value);
        }

        Ok(Some(serde_json::Value::Object(payload)))
    }

    /// Writes one live column row: the marker, then one field key per
    /// non-retired field. A field the document omits is stored as explicit
    /// SQL NULL so every required field key exists. Never writes a row blob.
    pub(crate) fn write_column_store_document_to_tx(
        tx: &mutcrate::adapter::AdapterTransaction,
        collection: &str,
        id: &str,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        let document = payload
            .as_object()
            .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;

        let revision = Self::next_column_store_revision(tx, collection, id, row_schema)?;
        tx.put(
            Self::column_store_row_key(row_schema.relation_id, id),
            encode_marker(row_schema.schema_version, revision),
            None,
        )
        .map_err(CassieError::from)?;
        tx.delete(Self::column_store_deleted_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?;

        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let key = Self::column_store_field_key(row_schema.relation_id, field.field_id, id);
            let encoded = super::compression::encode_column_field(
                &field.data_type,
                document.get(&field.name),
                true,
            )?;
            tx.put(key, encoded, None).map_err(CassieError::from)?;
        }
        Ok(())
    }

    pub(crate) fn delete_column_store_document_to_tx(
        tx: &mutcrate::adapter::AdapterTransaction,
        collection: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        let revision = Self::next_column_store_revision(tx, collection, id, row_schema)?;
        tx.delete(Self::column_store_row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?;
        tx.put(
            Self::column_store_deleted_key(row_schema.relation_id, id),
            encode_deleted_marker(revision),
            None,
        )
        .map_err(CassieError::from)?;
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            tx.delete(Self::column_store_field_key(
                row_schema.relation_id,
                field.field_id,
                id,
            ))
            .map_err(CassieError::from)?;
        }
        Ok(())
    }

    /// The write revision for the next change to `id`: one past the live
    /// marker's revision, or past the delete marker's when the row is absent,
    /// so a delete followed by a re-insert never repeats a marker value.
    fn next_column_store_revision(
        tx: & crate::adapter::AdapterTransaction,
        collection: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<u64, CassieError> {
        let previous = if let Some(marker) = tx
            .get(&Self::column_store_row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?
        {
            decode_marker_parts(collection, id, &marker)?.1
        } else {
            tx.get(&Self::column_store_deleted_key(row_schema.relation_id, id))
                .map_err(CassieError::from)?
                .map_or(0, |marker| decode_deleted_marker(&marker))
        };
        Ok(previous
            .max(row_schema.column_revision_floor)
            .saturating_add(1))
    }

    pub(crate) fn scan_column_store_rows_batched(
        &self,
        tx: & crate::adapter::AdapterTransaction,
        request: ColumnStoreScanRequest<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let scan_started = Instant::now();
        let mut row_decode = Duration::ZERO;
        let mut results = Vec::new();
        if request.limit == 0 {
            return Ok((
                results,
                MidgeScanTimings {
                    scan: scan_started.elapsed(),
                    row_decode,
                },
            ));
        }

        let selection = ColumnFieldSelection::new(request.row_schema, request.projection);
        if request.traversal == super::super::RowTraversal::CompleteSource {
            // D1: validate the fields this scan reads (selection plus the
            // filter field) and the markers, not every field of the table.
            let filter_field = request.filter.and_then(|filter| {
                let reference = cassie_sql::column_identifier::ColumnIdentifierPath::parse(&filter.field).ok()?;
                request
                    .row_schema
                    .fields
                    .iter()
                    .find(|field| !field.retired && reference.matches_field_name(&field.name))
            });
            self.validate_field_markers(
                tx,
                request.collection,
                request.row_schema,
                selection.fields(request.row_schema).chain(filter_field),
                || Ok(()),
            )?;
        }
        let batch_size = request.batch_size.max(1);
        let mut current = Vec::with_capacity(batch_size);
        let mut emitted = 0usize;
        let row_prefix = Self::column_store_row_prefix(request.row_schema.relation_id);
        let scan = collect_scan(
            tx.scan(&Query::new().prefix(row_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;

        for (raw_key, raw_marker) in scan {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &row_prefix) else {
                continue;
            };

            let decode_started = Instant::now();
            let payload =
                self.project_column_store_document(tx, &id, &raw_marker, &request, &selection)?;
            row_decode += decode_started.elapsed();
            let Some(payload) = payload else {
                continue;
            };

            current.push(DocumentRef { id, payload });
            emitted += 1;
            if current.len() >= batch_size {
                results.push(current);
                current = Vec::with_capacity(batch_size);
            }
            if emitted >= request.limit {
                break;
            }
        }

        if !current.is_empty() {
            results.push(current);
        }

        Ok((
            results,
            MidgeScanTimings {
                scan: scan_started.elapsed().saturating_sub(row_decode),
                row_decode,
            },
        ))
    }

    fn project_column_store_document(
        &self,
        tx: & crate::adapter::AdapterTransaction,
        id: &str,
        raw_marker: &[u8],
        request: &ColumnStoreScanRequest<'_>,
        selection: &ColumnFieldSelection,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        let row_schema = request.row_schema;
        let row = ColumnRowRef {
            relation: request.collection,
            id,
            row_schema,
            marker_version: decode_marker(request.collection, id, raw_marker)?,
        };
        if let Some(filter) = request.filter {
            if !self.column_store_row_matches_filter(tx, row, filter)? {
                return Ok(None);
            }
        }

        let mut object = serde_json::Map::new();
        for field in selection.fields(row_schema) {
            let Some(value) = self.read_counted_column_store_field(tx, row, field)? else {
                continue;
            };
            object.insert(field.name.clone(), value);
        }
        self.column_vector_counters().record_row_document();
        Ok(Some(serde_json::Value::Object(object)))
    }

    fn column_store_row_matches_filter(
        &self,
        tx: & crate::adapter::AdapterTransaction,
        row: ColumnRowRef<'_>,
        filter: &RowFilter,
    ) -> Result<bool, CassieError> {
        let Some(filter_field) = row.row_schema.fields.iter().find(|field| {
            !field.retired
                && cassie_sql::column_identifier::ColumnIdentifierPath::parse(&filter.field)
                    .is_ok_and(|reference| reference.matches_field_name(&field.name))
        }) else {
            return Ok(false);
        };
        let value = self.read_counted_column_store_field(tx, row, filter_field)?;
        Ok(value.is_some_and(|value| value == filter.value))
    }
}
