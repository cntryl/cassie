//! Authoritative column vector reader (#1358).
//!
//! A column row is one live marker plus one key per field. This reader turns a
//! range of rows into [`RawLanes`] batches of at most [`COLUMN_VECTOR_BATCH_ROWS`]
//! rows without ever building a row document: it walks the markers and one
//! range iterator per selected field in id order (see `merge`), and decodes
//! each stored value straight into its lane (see `decode`).
//!
//! Batches never cross a segment boundary: the caller supplies the id ranges to
//! read (one whole-table range, or the segments pruning did not exclude) and
//! each batch lies inside one range. Ownership validation covers exactly the
//! fields the scan reads plus the markers (D1); the merge finds an orphan in a
//! read field, and `cassie verify` finds the rest.

mod counters;
mod decode;
mod merge;
mod partitions;
#[cfg(feature = "storage-test-hooks")]
pub mod test_support;
#[cfg(test)]
mod tests;

pub use counters::ColumnVectorReadCounters;
pub use counters::ColumnVectorReadSnapshot;
pub use partitions::ColumnPartitionReader;

use super::validation::decode_marker;
use super::{CassieError, Midge, Query, RowSchema};
use crate::adapter::ColumnFieldSelection;
use crate::adapter::{key_encoding, DataReadTransaction, RowTraversal};
use crate::adapter::{QueryMemoryReservation, StorageRequestContext};
use crate::row_blob::RowFieldMeta;
use cassie_core::types::lanes::{LaneValidity, RawLanes};
use decode::{LaneBuilder, LaneKind};
use merge::{merge_batch, FieldStream, MergeInput};
use std::mem::size_of;

/// Rows per vector batch (owner decision D8).
pub const COLUMN_VECTOR_BATCH_ROWS: usize = 1024;

pub(super) fn check_controls(controls: &StorageRequestContext<'_>) -> Result<(), CassieError> {
    if controls.is_canceled() {
        return Err(CassieError::QueryCanceled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

/// A half-open range of row ids: `start` inclusive, `end` exclusive, `None`
/// meaning unbounded on that side.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnIdRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

/// A live marker of the current batch.
pub(super) struct Marker {
    pub id: String,
    pub version: u32,
}

/// The rows of one batch: ids, then one lane carrier per selected field.
#[derive(Debug)]
pub struct ColumnVectorBatch {
    pub ids: Vec<String>,
    pub lanes: Vec<RawLanes>,
    /// Admission of the decoded batch; drop it once a column retains the lanes.
    pub memory: QueryMemoryReservation,
}

/// What a scan reads and which ranges it covers.
pub(crate) struct ColumnVectorRequest<'a> {
    pub collection: &'a str,
    pub row_schema: &'a RowSchema,
    pub selection: &'a ColumnFieldSelection,
    pub ranges: Vec<ColumnIdRange>,
    pub traversal: RowTraversal,
    pub limit: Option<usize>,
}

/// Position of a vector scan. The transaction is supplied per call so the
/// owner can keep scan and snapshot together without borrowing itself.
pub(crate) struct ColumnVectorScan {
    collection: String,
    relation_id: u64,
    fields: Vec<RowFieldMeta>,
    kinds: Vec<LaneKind>,
    ranges: Vec<ColumnIdRange>,
    traversal: RowTraversal,
    remaining: Option<usize>,
    range_pos: usize,
    resume: Option<String>,
}

impl Midge {
    /// Opens a vector scan. Nothing is read until the first batch.
    pub(crate) fn open_column_vector_scan(
        &self,
        request: ColumnVectorRequest<'_>,
    ) -> ColumnVectorScan {
        self.column_vector_counters().record_scan();
        let fields = request
            .selection
            .fields(request.row_schema)
            .cloned()
            .collect::<Vec<_>>();
        let kinds = fields
            .iter()
            .map(|field| LaneKind::of(&field.data_type))
            .collect();
        ColumnVectorScan {
            collection: request.collection.to_owned(),
            relation_id: request.row_schema.relation_id,
            fields,
            kinds,
            ranges: request.ranges,
            traversal: request.traversal,
            remaining: request.limit,
            range_pos: 0,
            resume: None,
        }
    }
}

fn after(mut key: Vec<u8>) -> Vec<u8> {
    key.push(0);
    key
}

/// A range query over `[start, end)` inside `prefix`; returns the bytes of the
/// keys it built so the caller can admit them.
fn range_query(
    prefix: &[u8],
    start: Option<Vec<u8>>,
    end: Option<Vec<u8>>,
    limit: Option<usize>,
) -> (Query, usize) {
    let mut bytes = 0usize;
    let mut query = Query::new();
    let bounded = end.is_some();
    if let Some(end) = end {
        bytes += end.len();
        query = query.end_key(end.into());
    } else {
        bytes += prefix.len();
        query = query.prefix(prefix.to_vec().into());
    }
    if let Some(start) = start {
        bytes += start.len();
        query = query.start_key(start.into());
    } else if bounded {
        bytes += prefix.len();
        query = query.start_key(prefix.to_vec().into());
    }
    if let Some(limit) = limit {
        query = query.limit(limit);
    }
    (query, bytes)
}

impl ColumnVectorScan {
    /// Replaces the id ranges still to be read (segment pruning).
    pub(super) fn set_ranges(&mut self, ranges: Vec<ColumnIdRange>) {
        self.ranges = ranges;
        self.range_pos = 0;
        self.resume = None;
    }

    /// Reads the next batch of at most `max_rows` rows, or `None` when every
    /// range is exhausted.
    ///
    /// # Errors
    ///
    /// Returns `XX001` for corrupt column data, a resource-limit error when the
    /// query budget cannot hold the batch, and cancellation errors.
    pub fn next_batch(
        &mut self,
        midge: &Midge,
        tx: &DataReadTransaction,
        max_rows: usize,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Option<ColumnVectorBatch>, CassieError> {
        loop {
            check_controls(controls)?;
            let want = COLUMN_VECTOR_BATCH_ROWS
                .min(max_rows.max(1))
                .min(self.remaining.unwrap_or(usize::MAX));
            if self.range_pos >= self.ranges.len() || want == 0 {
                return Ok(None);
            }
            if let Some(batch) = self.read_batch(midge, tx, want, controls)? {
                return Ok(Some(batch));
            }
        }
    }

    fn read_batch(
        &mut self,
        midge: &Midge,
        tx: &DataReadTransaction,
        want: usize,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Option<ColumnVectorBatch>, CassieError> {
        let mut memory = controls.reserve_query_memory(0)?;
        let (markers, full) = self.read_markers(midge, tx, want, controls, &mut memory)?;
        let rows = markers.len();
        let lane_bytes = self
            .kinds
            .iter()
            .map(|kind| kind.lane_bytes())
            .sum::<usize>();
        memory.try_grow(rows.saturating_mul(lane_bytes))?;
        let mut builders = self
            .kinds
            .iter()
            .map(|kind| LaneBuilder::new(*kind, rows))
            .collect::<Vec<_>>();
        let last = markers.last().map(|marker| marker.id.clone());
        let full = full && last.is_some();
        let mut streams = self.field_streams(
            tx,
            full.then_some(last.as_deref()).flatten(),
            controls,
            &mut memory,
        )?;
        merge_batch(MergeInput {
            midge,
            relation: &self.collection,
            fields: &self.fields,
            markers: &markers,
            traversal: self.traversal,
            controls,
            streams: &mut streams,
            builders: &mut builders,
            memory: &mut memory,
        })?;
        drop(streams);
        if full {
            self.resume = last;
        } else {
            self.range_pos += 1;
            self.resume = None;
        }
        if let Some(remaining) = self.remaining.as_mut() {
            *remaining = remaining.saturating_sub(rows);
        }
        if rows == 0 {
            return Ok(None);
        }
        let ids = markers.into_iter().map(|marker| marker.id).collect();
        let lanes = builders
            .into_iter()
            .map(LaneBuilder::finish)
            .collect::<Vec<_>>();
        midge
            .column_vector_counters()
            .record_batch(rows, lanes.len());
        Ok(Some(ColumnVectorBatch { ids, lanes, memory }))
    }

    /// Reads up to `want` markers of the current range after the resume point.
    /// The flag says the read hit `want`, so more markers may follow.
    fn read_markers(
        &self,
        midge: &Midge,
        tx: &DataReadTransaction,
        want: usize,
        controls: &StorageRequestContext<'_>,
        memory: &mut QueryMemoryReservation,
    ) -> Result<(Vec<Marker>, bool), CassieError> {
        let range = &self.ranges[self.range_pos];
        let prefix = Midge::column_store_row_prefix(self.relation_id);
        let start = match (&self.resume, &range.start) {
            (Some(id), _) => Some(after(Midge::column_store_row_key(self.relation_id, id))),
            (None, Some(start)) => Some(Midge::column_store_row_key(self.relation_id, start)),
            (None, None) => None,
        };
        let end = range
            .end
            .as_ref()
            .map(|end| Midge::column_store_row_key(self.relation_id, end));
        let (query, key_bytes) = range_query(&prefix, start, end, Some(want));
        memory.try_grow(key_bytes)?;
        let mut markers = Vec::new();
        let mut seen = 0usize;
        for entry in tx.scan(&query).map_err(CassieError::from)? {
            check_controls(controls)?;
            let (key, value) = entry.map_err(CassieError::from)?;
            seen += 1;
            midge.record_query_scan_entry();
            if crate::adapter::query_scan_control::should_cancel_controlled_query_scan(controls) {
                return Err(CassieError::QueryCanceled);
            }
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let version = decode_marker(&self.collection, &id, &value)?;
            memory.try_grow(size_of::<Marker>().saturating_add(id.len()))?;
            markers.try_reserve(1).map_err(|error| {
                CassieError::ResourceLimit(format!(
                    "unable to retain column vector markers: {error}"
                ))
            })?;
            markers.push(Marker { id, version });
        }
        Ok((markers, seen >= want))
    }

    fn field_streams<'tx>(
        &self,
        tx: &'tx DataReadTransaction,
        bound: Option<&str>,
        controls: &StorageRequestContext<'_>,
        memory: &mut QueryMemoryReservation,
    ) -> Result<Vec<FieldStream<'tx>>, CassieError> {
        let range = &self.ranges[self.range_pos];
        let mut streams = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            check_controls(controls)?;
            let field_key =
                |id: &str| Midge::column_store_field_key(self.relation_id, field.field_id, id);
            let prefix = Midge::column_store_field_prefix(self.relation_id, field.field_id);
            let start = match (&self.resume, &range.start) {
                (Some(id), _) => Some(after(field_key(id))),
                (None, Some(start)) => Some(field_key(start)),
                (None, None) => None,
            };
            let end = match bound {
                Some(last) => Some(after(field_key(last))),
                None => range.end.as_deref().map(field_key),
            };
            let (query, key_bytes) = range_query(&prefix, start, end, None);
            memory.try_grow(key_bytes)?;
            let iter = tx.scan(&query).map_err(CassieError::from)?;
            streams.push(FieldStream::new(iter, prefix));
        }
        Ok(streams)
    }
}

/// Where a requested output column comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnVectorOutput {
    /// The row identity, as text.
    Identity,
    /// The lane at this index of [`ColumnVectorBatch::lanes`].
    Lane(usize),
}

/// A vector scan together with the snapshot it reads and its output binding.
pub struct ColumnVectorReader {
    tx: DataReadTransaction,
    scan: ColumnVectorScan,
    outputs: Vec<ColumnVectorOutput>,
    row_schema: RowSchema,
}

impl ColumnVectorReader {
    /// Skips segments whose verified summaries exclude `filter`. Segments that
    /// hold a staged id of the open transaction are always read. An untrusted
    /// summary, a table without summaries, or a failed summary read leaves the
    /// whole table in scope, so pruning can only remove work.
    pub fn set_pruning(
        &mut self,
        midge: &Midge,
        filter: &crate::adapter::ColumnBatchScanFilter,
        staged: &[&str],
        controls: &StorageRequestContext<'_>,
    ) {
        if !midge.column_pruning_enabled() {
            return;
        }
        let Some(mut pruning) =
            super::ColumnPruning::resolve(&self.scan.collection, &self.row_schema, filter)
        else {
            return;
        };
        if let Ok(Some(ranges)) = pruning.plan_ranges(midge, &self.tx, staged, controls) {
            self.scan.set_ranges(ranges);
        }
    }

    pub fn outputs(&self) -> &[ColumnVectorOutput] {
        &self.outputs
    }

    /// The next batch of at most `max_rows` rows.
    ///
    /// # Errors
    ///
    /// See [`ColumnVectorScan::next_batch`].
    pub fn next_batch(
        &mut self,
        midge: &Midge,
        max_rows: usize,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Option<ColumnVectorBatch>, CassieError> {
        self.scan.next_batch(midge, &self.tx, max_rows, controls)
    }
}

impl Midge {
    /// Opens a vector reader over a column-authoritative table.
    ///
    /// Each requested output is `None` for the row identity or the name of a
    /// stored field; `Ok(None)` means a name is not a plain stored field and
    /// the caller reads documents instead.
    ///
    /// # Errors
    ///
    /// Returns storage errors; an unknown collection is an error.
    pub fn open_column_vector_reader(
        &self,
        collection: &str,
        requested: &[Option<&str>],
        traversal: RowTraversal,
    ) -> Result<Option<ColumnVectorReader>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        if !self.collection_uses_column_store(&collection)? {
            return Ok(None);
        }
        let row_schema = self.row_schema(&collection)?;
        let mut selected = std::collections::HashSet::new();
        let mut order: Vec<usize> = Vec::new();
        let mut outputs = Vec::with_capacity(requested.len());
        for name in requested {
            let Some(name) = name else {
                outputs.push(ColumnVectorOutput::Identity);
                continue;
            };
            let Ok(reference) = cassie_sql::column_identifier::ColumnIdentifierPath::parse(name)
            else {
                return Ok(None);
            };
            let Some(index) = row_schema
                .fields
                .iter()
                .position(|field| !field.retired && reference.matches_field_name(&field.name))
            else {
                return Ok(None);
            };
            if selected.insert(index) {
                order.push(index);
            }
            outputs.push(ColumnVectorOutput::Lane(index));
        }
        // Lanes follow schema order, like `ColumnFieldSelection`.
        order.sort_unstable();
        for output in &mut outputs {
            if let ColumnVectorOutput::Lane(index) = output {
                *output = ColumnVectorOutput::Lane(
                    order
                        .iter()
                        .position(|candidate| candidate == index)
                        .unwrap_or(0),
                );
            }
        }
        let projection = order
            .iter()
            .map(|index| {
                cassie_sql::column_identifier::ColumnIdentifierPath::from_field_name(
                    &row_schema.fields[*index].name,
                )
                .lookup_key()
            })
            .collect::<std::collections::HashSet<String>>();
        let selection = ColumnFieldSelection::new(&row_schema, Some(&projection));
        let scan = self.open_column_vector_scan(ColumnVectorRequest {
            collection: &collection,
            row_schema: &row_schema,
            selection: &selection,
            ranges: vec![ColumnIdRange::default()],
            traversal,
            limit: None,
        });
        Ok(Some(ColumnVectorReader {
            tx: self.begin_data_readonly_tx_for(&collection)?,
            scan,
            outputs,
            row_schema,
        }))
    }
}

/// Text lanes carrying the row ids of a batch.
pub fn identity_lanes(ids: &[String]) -> RawLanes {
    let mut bytes = String::with_capacity(ids.iter().map(String::len).sum());
    let mut offsets = Vec::with_capacity(ids.len() + 1);
    offsets.push(0);
    for id in ids {
        bytes.push_str(id);
        offsets.push(bytes.len());
    }
    RawLanes::Utf8 {
        bytes,
        offsets,
        validity: LaneValidity::AllValid,
    }
}
