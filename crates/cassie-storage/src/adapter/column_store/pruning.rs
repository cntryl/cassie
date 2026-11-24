//! Reader side of segment pruning for column-authoritative scans (#1357).
//!
//! A scan resolves its conjunctive filter into [`ColumnPruning`]. When the scan
//! reaches the first live marker of a segment whose summary excludes the
//! filter, it re-reads that segment's markers (never a field payload) and
//! compares their fingerprint with the record's. Only an exact match skips the
//! segment; a missing, malformed, stale or mismatching record means the
//! segment is read normally, so a summary can never hide a qualifying row.

use super::summary::{
    segment_excluded, MarkerFingerprint, PruneOp, PrunePredicate, SummaryKind, SummaryRecord,
};
use super::{CassieError, Midge, Query, RowSchema};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Process-lifetime counters of pruning work on column tables.
#[derive(Debug, Default)]
pub(crate) struct ColumnPruningCounters {
    segments_skipped: AtomicU64,
    rows_skipped: AtomicU64,
    summaries_loaded: AtomicU64,
    summaries_rejected: AtomicU64,
    summaries_stale: AtomicU64,
    fingerprint_markers: AtomicU64,
}

impl ColumnPruningCounters {
    pub(crate) fn snapshot(&self) -> ColumnPruningSnapshot {
        ColumnPruningSnapshot {
            segments_skipped: self.segments_skipped.load(Ordering::Relaxed),
            rows_skipped: self.rows_skipped.load(Ordering::Relaxed),
            summaries_loaded: self.summaries_loaded.load(Ordering::Relaxed),
            summaries_rejected: self.summaries_rejected.load(Ordering::Relaxed),
            summaries_stale: self.summaries_stale.load(Ordering::Relaxed),
            fingerprint_markers: self.fingerprint_markers.load(Ordering::Relaxed),
        }
    }

    fn add(counter: &AtomicU64, amount: u64) {
        counter.fetch_add(amount, Ordering::Relaxed);
    }
}

/// Pruning state shared by every scan of one store: counters, the diagnostic
/// switch that forces unpruned reads, and tables whose summaries a scan found
/// stale and the next maintenance pass should rebuild completely.
#[derive(Debug, Default)]
pub(crate) struct ColumnPruningState {
    counters: ColumnPruningCounters,
    disabled: AtomicBool,
    stale_tables: parking_lot::Mutex<HashSet<String>>,
}

impl Midge {
    fn column_pruning(&self) -> &ColumnPruningCounters {
        &self.column_pruning.counters
    }

    pub(crate) fn column_pruning_enabled(&self) -> bool {
        !self.column_pruning.disabled.load(Ordering::Relaxed)
    }

    /// Skip, stale and rejection counts of segment pruning on column tables.
    #[doc(hidden)]
    #[must_use]
    pub fn column_pruning_for_diagnostics(&self) -> ColumnPruningSnapshot {
        self.column_pruning.counters.snapshot()
    }

    /// Forces column scans to read every segment (`false`) or restores pruning.
    #[doc(hidden)]
    pub fn set_column_pruning_enabled_for_diagnostics(&self, enabled: bool) {
        self.column_pruning
            .disabled
            .store(!enabled, Ordering::Relaxed);
    }

    pub(super) fn note_stale_column_summaries(&self, collection: &str) {
        self.column_pruning
            .stale_tables
            .lock()
            .insert(collection.to_owned());
    }

    pub(super) fn take_stale_column_summaries(&self, collection: &str) -> bool {
        self.column_pruning.stale_tables.lock().remove(collection)
    }
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_canceled() {
        return Err(CassieError::QueryCanceled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

/// A point-in-time copy of the pruning counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnPruningSnapshot {
    /// Segments skipped without reading any field payload.
    pub segments_skipped: u64,
    /// Live rows inside skipped segments.
    pub rows_skipped: u64,
    /// Valid summary records loaded by scans.
    pub summaries_loaded: u64,
    /// Records ignored because they were malformed, corrupt or of an unknown version.
    pub summaries_rejected: u64,
    /// Skip candidates refused because the live markers no longer matched the record.
    pub summaries_stale: u64,
    /// Live markers re-read to verify skip candidates (never field payloads).
    pub fingerprint_markers: u64,
}

impl ColumnPruningSnapshot {
    /// The work done since `earlier`.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            segments_skipped: self
                .segments_skipped
                .saturating_sub(earlier.segments_skipped),
            rows_skipped: self.rows_skipped.saturating_sub(earlier.rows_skipped),
            summaries_loaded: self
                .summaries_loaded
                .saturating_sub(earlier.summaries_loaded),
            summaries_rejected: self
                .summaries_rejected
                .saturating_sub(earlier.summaries_rejected),
            summaries_stale: self.summaries_stale.saturating_sub(earlier.summaries_stale),
            fingerprint_markers: self
                .fingerprint_markers
                .saturating_sub(earlier.fingerprint_markers),
        }
    }
}

/// A stored summary slot: its segment's lower bound and its record, when valid.
pub(super) struct LoadedRecord {
    pub storage_key: Vec<u8>,
    pub start: Option<String>,
    pub record: Option<SummaryRecord>,
    pub value_bytes: usize,
}

/// Reads every summary slot of a table in segment order. A slot whose value
/// does not decode, does not match its key, or has an unknown version keeps its
/// place with `record: None`, so neighbouring ranges are never extended over it.
pub(super) fn load_records(
    tx: & crate::adapter::DataReadTransaction,
    relation_id: u64,
) -> Result<Vec<LoadedRecord>, CassieError> {
    let prefix = Midge::column_store_summary_prefix(relation_id);
    let mut loaded = Vec::new();
    for entry in tx
        .scan(&Query::new().prefix(prefix.clone().into()))
        .map_err(CassieError::from)?
    {
        let (key, value) = entry.map_err(CassieError::from)?;
        let Some(suffix) = key.strip_prefix(prefix.as_slice()) else {
            continue;
        };
        let start = match suffix.split_first() {
            Some((1, [])) => None,
            Some((2, id)) => match std::str::from_utf8(id) {
                Ok(id) => Some(id.to_owned()),
                Err(_) => continue,
            },
            _ => continue,
        };
        let record = SummaryRecord::decode(&value)
            .filter(|record| record.start == start.as_deref().unwrap_or_default());
        loaded.push(LoadedRecord {
            storage_key: key.to_vec(),
            start,
            record,
            value_bytes: value.len(),
        });
    }
    Ok(loaded)
}

struct SegmentIndex {
    lower_bounds: Vec<Vec<u8>>,
    starts: Vec<Option<String>>,
    records: Vec<Option<SummaryRecord>>,
    _memory: QueryMemoryReservation,
}

/// A scan's resolved filter and its lazily loaded view of the summaries.
pub(crate) struct ColumnPruning {
    collection: String,
    relation_id: u64,
    schema_version: u32,
    predicates: Vec<PrunePredicate>,
    index: Option<SegmentIndex>,
    current_segment: Option<usize>,
}

impl std::fmt::Debug for ColumnPruning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ColumnPruning")
            .field("predicates", &self.predicates.len())
            .field("loaded", &self.index.is_some())
            .finish_non_exhaustive()
    }
}

impl ColumnPruning {
    /// Resolves a conjunctive scan filter against the table's schema. Returns
    /// `None` when no conjunct can be checked against a summarized field.
    pub(crate) fn resolve(
        collection: &str,
        row_schema: &RowSchema,
        filter: & crate::adapter::ColumnBatchScanFilter,
    ) -> Option<Self> {
        let predicates = filter
            .predicates
            .iter()
            .filter_map(|predicate| resolve_predicate(row_schema, predicate))
            .collect::<Vec<_>>();
        (!predicates.is_empty()).then(|| Self {
            collection: collection.to_owned(),
            relation_id: row_schema.relation_id,
            schema_version: row_schema.schema_version,
            predicates,
            index: None,
            current_segment: None,
        })
    }

    /// Decides, at a live marker, whether the segment it opens can be skipped.
    ///
    /// Returns the key of the segment's last live marker when the whole segment
    /// is excluded and its markers still match the record exactly; the scan
    /// resumes after it. `None` means the scan reads the segment normally.
    pub(incrate::adapter) fn skip_to(
        &mut self,
        midge: &Midge,
        tx: & crate::adapter::DataReadTransaction,
        marker_key: &[u8],
        controls: &QueryExecutionControls,
    ) -> Result<Option<Vec<u8>>, CassieError> {
        if self.index.is_none() {
            self.index = Some(self.load_index(midge, tx, controls)?);
        }
        let Some(index) = self.index.as_ref() else {
            return Ok(None);
        };
        let position = index
            .lower_bounds
            .partition_point(|lower| lower.as_slice() <= marker_key);
        let Some(segment) = position.checked_sub(1) else {
            return Ok(None);
        };
        if self.current_segment == Some(segment) {
            return Ok(None);
        }
        self.current_segment = Some(segment);
        let Some(Some(record)) = index.records.get(segment) else {
            return Ok(None);
        };
        let counters = midge.column_pruning();
        if record.schema_version != self.schema_version {
            ColumnPruningCounters::add(&counters.summaries_stale, 1);
            midge.note_stale_column_summaries(&self.collection);
            return Ok(None);
        }
        if !segment_excluded(record, &self.predicates) {
            return Ok(None);
        }

        self.verified_last_key(midge, tx, segment, controls)
    }

    /// Re-reads an excluded segment's markers and compares their fingerprint
    /// with its record. Returns the segment's last live marker key only when
    /// the record is trusted; a mismatch counts as stale and reads normally.
    fn verified_last_key(
        &self,
        midge: &Midge,
        tx: & crate::adapter::DataReadTransaction,
        segment: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Vec<u8>>, CassieError> {
        let Some(index) = self.index.as_ref() else {
            return Ok(None);
        };
        let Some(Some(record)) = index.records.get(segment) else {
            return Ok(None);
        };
        let counters = midge.column_pruning();
        let mut query = Query::new().start_key(index.lower_bounds[segment].clone().into());
        query = match index.lower_bounds.get(segment + 1) {
            Some(end) => query.end_key(end.clone().into()),
            None => query.prefix(Midge::column_store_row_prefix(self.relation_id).into()),
        };
        let mut fingerprint = MarkerFingerprint::new();
        let mut last_key = None;
        for entry in tx.scan(&query).map_err(CassieError::from)? {
            check_controls(controls)?;
            let (key, marker) = entry.map_err(CassieError::from)?;
            fingerprint.add(&key, &marker);
            last_key = Some(key.to_vec());
        }
        let (count, digest) = fingerprint.finish();
        ColumnPruningCounters::add(&counters.fingerprint_markers, u64::from(count));
        if count != record.row_count || digest != record.fingerprint {
            ColumnPruningCounters::add(&counters.summaries_stale, 1);
            midge.note_stale_column_summaries(&self.collection);
            return Ok(None);
        }
        ColumnPruningCounters::add(&counters.segments_skipped, 1);
        ColumnPruningCounters::add(&counters.rows_skipped, u64::from(count));
        Ok(last_key)
    }

    /// Plans the id ranges a vector scan reads: one per segment, omitting each
    /// segment whose verified summary excludes the filter. A segment that holds
    /// an id the open transaction staged is never omitted, and a summary that
    /// cannot be trusted leaves its segment in. `None` means the table has no
    /// summaries, so the scan reads one whole range.
    pub(incrate::adapter) fn plan_ranges(
        &mut self,
        midge: &Midge,
        tx: & crate::adapter::DataReadTransaction,
        staged: &[&str],
        controls: &QueryExecutionControls,
    ) -> Result<Option<Vec<super::ColumnIdRange>>, CassieError> {
        if self.index.is_none() {
            self.index = Some(self.load_index(midge, tx, controls)?);
        }
        let Some(index) = self.index.as_ref() else {
            return Ok(None);
        };
        if index.records.is_empty() {
            return Ok(None);
        }
        let segments = index.records.len();
        let mut ranges = Vec::with_capacity(segments);
        let (mut pruned, mut untrusted) = (0_u64, 0_u64);
        for segment in 0..segments {
            check_controls(controls)?;
            let start = index.starts[segment].clone();
            let end = index.starts.get(segment + 1).cloned().flatten();
            let holds_staged = staged.iter().any(|id| {
                start.as_deref().is_none_or(|start| *id >= start)
                    && end.as_deref().is_none_or(|end| *id < end)
            });
            let record = index.records[segment].as_ref();
            let excluded = !holds_staged
                && record.is_some_and(|record| {
                    record.schema_version == self.schema_version
                        && segment_excluded(record, &self.predicates)
                });
            if excluded {
                if self
                    .verified_last_key(midge, tx, segment, controls)?
                    .is_some()
                {
                    pruned += 1;
                    continue;
                }
                untrusted += 1;
            }
            ranges.push(super::ColumnIdRange { start, end });
        }
        midge
            .column_vector_counters()
            .record_segments(pruned, untrusted);
        Ok(Some(ranges))
    }

    fn load_index(
        &self,
        midge: &Midge,
        tx: & crate::adapter::DataReadTransaction,
        controls: &QueryExecutionControls,
    ) -> Result<SegmentIndex, CassieError> {
        let loaded = load_records(tx, self.relation_id)?;
        let bytes = loaded.iter().map(|slot| slot.value_bytes).sum::<usize>();
        let memory = controls.reserve_query_memory(bytes)?;
        let counters = midge.column_pruning();
        let valid = loaded.iter().filter(|slot| slot.record.is_some()).count();
        ColumnPruningCounters::add(&counters.summaries_loaded, valid as u64);
        ColumnPruningCounters::add(&counters.summaries_rejected, (loaded.len() - valid) as u64);
        let lower_bounds = loaded
            .iter()
            .map(|slot| match slot.start.as_deref() {
                None => Midge::column_store_row_prefix(self.relation_id),
                Some(id) => Midge::column_store_row_key(self.relation_id, id),
            })
            .collect();
        let starts = loaded.iter().map(|slot| slot.start.clone()).collect();
        let records = loaded.into_iter().map(|slot| slot.record).collect();
        Ok(SegmentIndex {
            lower_bounds,
            starts,
            records,
            _memory: memory,
        })
    }
}

fn resolve_predicate(
    row_schema: &RowSchema,
    predicate: & crate::adapter::ColumnBatchScanPredicate,
) -> Option<PrunePredicate> {
    use crate::adapter::ColumnBatchScanOp as Op;
    let reference = cassie_sql::column_identifier::ColumnIdentifierPath::parse(&predicate.field).ok()?;
    let field = row_schema
        .fields
        .iter()
        .find(|field| !field.retired && reference.matches_field_name(&field.name))?;
    let kind = SummaryKind::of(&field.data_type)?;
    let op = match predicate.op {
        Op::Eq => PruneOp::Eq,
        Op::Lt => PruneOp::Lt,
        Op::Lte => PruneOp::Lte,
        Op::Gt => PruneOp::Gt,
        Op::Gte => PruneOp::Gte,
        Op::IsNull => PruneOp::IsNull,
        Op::IsNotNull => PruneOp::IsNotNull,
    };
    let literal = match op {
        PruneOp::IsNull | PruneOp::IsNotNull => None,
        _ => Some(kind.literal(predicate.value.as_ref()?)?),
    };
    Some(PrunePredicate {
        field_id: field.field_id,
        kind,
        op,
        literal,
    })
}
