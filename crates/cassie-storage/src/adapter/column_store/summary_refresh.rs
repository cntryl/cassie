//! Maintenance of the pruning summaries of column-authoritative tables (#1357).
//!
//! Summaries are rebuilt after a committed write, in a transaction of their
//! own. A crash or failure between the write and the refresh only leaves the
//! affected records stale, which readers detect from the marker fingerprint and
//! bypass. The refresh reads one snapshot, so each record's fingerprint and
//! bounds describe the same rows.

use super::pruning::{load_records, LoadedRecord};
use super::summary::{
    FieldSummary, MarkerFingerprint, SummaryKind, SummaryRecord, SEGMENT_ROWS, SPLIT_ROWS,
};
use super::validation::{decode_field, decode_marker_parts, field_is_required, missing_field};
use super::{CassieError, Midge, Query, RowSchema};
use cassie_core::types::semantic::compare_values;
use cassie_core::types::Value;
use std::collections::BTreeSet;

type Marker = (Vec<u8>, Vec<u8>);

impl Midge {
    /// Rebuilds the summaries a commit made stale, or all of them.
    ///
    /// With `changed_ids`, only the segments those rows fall into are rebuilt,
    /// unless nothing usable exists yet, a record is malformed or from another
    /// schema version, or a scan reported stale records; then every segment is
    /// rebuilt. A failure leaves the old records, which readers verify anyway.
    ///
    /// # Errors
    ///
    /// Returns an error when the snapshot cannot be read, a field of a
    /// summarized row is corrupt, or the summaries cannot be committed.
    pub fn refresh_column_store_summaries(
        &self,
        collection: &str,
        changed_ids: Option<&[String]>,
    ) -> Result<usize, CassieError> {
        let collection = self.canonical_collection_name(collection);
        if !self.collection_uses_column_store(&collection)? {
            return Ok(0);
        }
        let _source = super::super::commit_source::CommitSourceScope::enter(
            super::super::commit_source::CommitSource::ColumnMaintenance,
        );
        let row_schema = self.row_schema(&collection)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let existing = load_records(&tx, row_schema.relation_id)?;
        let rebuild_all = self.take_stale_column_summaries(&collection)
            || summaries_unusable(&existing, &row_schema)
            || changed_ids.is_none();
        let ranges = if rebuild_all {
            vec![(None, None)]
        } else {
            dirty_ranges(&existing, &row_schema, changed_ids.unwrap_or_default())
        };

        let mut records = Vec::new();
        for (start, end) in ranges {
            records.extend(Self::build_segment_records(
                &tx,
                &collection,
                &row_schema,
                (start.as_deref(), end.as_deref()),
            )?);
        }

        let mut write = self.begin_data_rw_tx_for(&collection)?;
        if rebuild_all {
            for slot in &existing {
                write
                    .delete(slot.storage_key.clone())
                    .map_err(CassieError::from)?;
            }
        }
        for record in &records {
            let start = (!record.start.is_empty()).then_some(record.start.as_str());
            write
                .put(
                    Self::column_store_summary_key(row_schema.relation_id, start),
                    record.encode(),
                    None,
                )
                .map_err(CassieError::from)?;
        }
        write
            .commit(self.write_options_buffered())
            .map_err(CassieError::from)?;
        Ok(records.len())
    }

    /// Rebuilds every segment summary of `collection`; returns the record count.
    ///
    /// # Errors
    ///
    /// Returns an error when the table cannot be read or the records committed.
    #[doc(hidden)]
    pub fn rebuild_column_store_summaries_for_diagnostics(
        &self,
        collection: &str,
    ) -> Result<usize, CassieError> {
        self.refresh_column_store_summaries(collection, None)
    }

    /// The raw key prefix of a column table's summary records.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection does not exist.
    #[doc(hidden)]
    pub fn column_store_summary_prefix_for_diagnostics(
        &self,
        collection: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let relation_id = self
            .collection_metadata(collection)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?
            .storage_id;
        Ok(Self::column_store_summary_prefix(relation_id))
    }

    fn build_segment_records(
        tx: &crate::adapter::DataReadTransaction,
        collection: &str,
        row_schema: &RowSchema,
        range: (Option<&str>, Option<&str>),
    ) -> Result<Vec<SummaryRecord>, CassieError> {
        let (start, end) = range;
        let row_prefix = Self::column_store_row_prefix(row_schema.relation_id);
        let lower = start.map_or_else(
            || row_prefix.clone(),
            |id| Self::column_store_row_key(row_schema.relation_id, id),
        );
        let mut query = Query::new().start_key(lower.into());
        query = match end {
            Some(id) => {
                query.end_key(Self::column_store_row_key(row_schema.relation_id, id).into())
            }
            None => query.prefix(row_prefix.clone().into()),
        };
        let mut records = Vec::new();
        let mut pending: Vec<Marker> = Vec::new();
        let mut next_start = start.map(str::to_owned);
        for entry in tx.scan(&query).map_err(CassieError::from)? {
            let (key, marker) = entry.map_err(CassieError::from)?;
            pending.push((key.to_vec(), marker.to_vec()));
            if pending.len() > SPLIT_ROWS {
                let rest = pending.split_off(SEGMENT_ROWS);
                let segment_start = next_start.take().unwrap_or_default();
                records.push(Self::build_record(
                    tx,
                    collection,
                    row_schema,
                    segment_start,
                    &pending,
                )?);
                next_start = rest
                    .first()
                    .and_then(|(key, _)| key.strip_prefix(row_prefix.as_slice()))
                    .map(|id| String::from_utf8_lossy(id).into_owned());
                pending = rest;
            }
        }
        let segment_start = next_start.unwrap_or_default();
        records.push(Self::build_record(
            tx,
            collection,
            row_schema,
            segment_start,
            &pending,
        )?);
        Ok(records)
    }

    fn build_record(
        tx: &crate::adapter::DataReadTransaction,
        collection: &str,
        row_schema: &RowSchema,
        start: String,
        markers: &[Marker],
    ) -> Result<SummaryRecord, CassieError> {
        let mut fingerprint = MarkerFingerprint::new();
        for (key, marker) in markers {
            fingerprint.add(key, marker);
        }
        let (row_count, digest) = fingerprint.finish();
        let row_prefix = Self::column_store_row_prefix(row_schema.relation_id);
        let mut fields = Vec::new();
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let Some(kind) = SummaryKind::of(&field.data_type) else {
                continue;
            };
            let mut summary = FieldSummary {
                field_id: field.field_id,
                kind,
                non_null: 0,
                min: None,
                max: None,
            };
            let mut complete = true;
            for (key, marker) in markers {
                let id = key
                    .strip_prefix(row_prefix.as_slice())
                    .and_then(|id| std::str::from_utf8(id).ok())
                    .ok_or_else(|| {
                        CassieError::Execution("invalid column marker key".to_owned())
                    })?;
                let (version, _) = decode_marker_parts(collection, id, marker)?;
                let raw = tx
                    .get(&Self::column_store_field_key(
                        row_schema.relation_id,
                        field.field_id,
                        id,
                    ))
                    .map_err(CassieError::from)?;
                let Some(raw) = raw else {
                    if field_is_required(field, version) {
                        return Err(missing_field(collection, id, field));
                    }
                    complete = false;
                    break;
                };
                match decode_field(collection, id, field, &raw)? {
                    None => {}
                    Some(value) if value.is_null() => {}
                    Some(value) => {
                        let Some(scalar) = kind.scalar(&value) else {
                            complete = false;
                            break;
                        };
                        summary.non_null += 1;
                        widen(&mut summary, scalar);
                    }
                }
            }
            if complete {
                fields.push(summary);
            }
        }
        Ok(SummaryRecord {
            start,
            schema_version: row_schema.schema_version,
            row_count,
            fingerprint: digest,
            fields,
        })
    }
}

fn widen(summary: &mut FieldSummary, scalar: Value) {
    if summary
        .min
        .as_ref()
        .is_none_or(|current| compare_values(&scalar, current).is_lt())
    {
        summary.min = Some(scalar.clone());
    }
    if summary
        .max
        .as_ref()
        .is_none_or(|current| compare_values(&scalar, current).is_gt())
    {
        summary.max = Some(scalar);
    }
}

/// Whether the stored records cannot be extended incrementally.
fn summaries_unusable(existing: &[LoadedRecord], row_schema: &RowSchema) -> bool {
    existing.first().is_none_or(|first| first.start.is_some())
        || existing.iter().any(|slot| {
            slot.record
                .as_ref()
                .is_none_or(|record| record.schema_version != row_schema.schema_version)
        })
}

/// The `[start, end)` segment ranges that contain a changed row.
fn dirty_ranges(
    existing: &[LoadedRecord],
    row_schema: &RowSchema,
    changed_ids: &[String],
) -> Vec<(Option<String>, Option<String>)> {
    let lower_bounds = existing
        .iter()
        .map(|slot| match slot.start.as_deref() {
            None => Midge::column_store_row_prefix(row_schema.relation_id),
            Some(id) => Midge::column_store_row_key(row_schema.relation_id, id),
        })
        .collect::<Vec<_>>();
    let dirty = changed_ids
        .iter()
        .filter_map(|id| {
            let key = Midge::column_store_row_key(row_schema.relation_id, id);
            lower_bounds
                .partition_point(|lower| lower.as_slice() <= key.as_slice())
                .checked_sub(1)
        })
        .collect::<BTreeSet<_>>();
    dirty
        .into_iter()
        .map(|index| {
            (
                existing[index].start.clone(),
                existing.get(index + 1).and_then(|next| next.start.clone()),
            )
        })
        .collect()
}
