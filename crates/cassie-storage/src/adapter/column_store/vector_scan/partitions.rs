//! Id-range partitions of a vector reader (#1358, parallel waves).
//!
//! A partition is one contiguous id range. Ranges come from segment pruning
//! (`plan_ranges`) or, for an unpruned scan, from the segment lower bounds of
//! the summary slots. Contiguous ranges always cover the whole id space, so a
//! stale segment boundary is still a valid partition: only pruning needs
//! trusted summaries. Every partition reads through the reader's own
//! transaction, so all partitions see the one statement snapshot.

use super::{
    check_controls, CassieError, ColumnIdRange, ColumnVectorBatch, ColumnVectorReader,
    ColumnVectorScan, Midge,
};
use crate::adapter::DataReadTransaction;
use crate::adapter::StorageRequestContext;

impl ColumnVectorScan {
    fn for_range(&self, range: ColumnIdRange) -> Self {
        Self {
            collection: self.collection.clone(),
            relation_id: self.relation_id,
            fields: self.fields.clone(),
            kinds: self.kinds.clone(),
            ranges: vec![range],
            traversal: self.traversal,
            remaining: None,
            range_pos: 0,
            resume: None,
        }
    }
}

/// One partition of a reader, bound to the reader's snapshot.
pub(crate) struct ColumnPartitionReader<'reader> {
    scan: ColumnVectorScan,
    tx: &'reader DataReadTransaction,
}

impl ColumnPartitionReader<'_> {
    /// The next batch of at most `max_rows` rows of this partition.
    ///
    /// # Errors
    ///
    /// See [`ColumnVectorScan::next_batch`].
    pub(crate) fn next_batch(
        &mut self,
        midge: &Midge,
        max_rows: usize,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Option<ColumnVectorBatch>, CassieError> {
        self.scan.next_batch(midge, self.tx, max_rows, controls)
    }
}

impl ColumnVectorReader {
    /// The id ranges still to be read, in id order.
    pub(crate) fn partition_ranges(&self) -> &[ColumnIdRange] {
        &self.scan.ranges
    }

    /// Splits an unpruned whole-table scan at the segment boundaries of the
    /// table's summary slots. A table without summaries stays one range.
    ///
    /// # Errors
    ///
    /// Returns storage errors, resource-limit errors and cancellation errors.
    pub(crate) fn split_by_segments(
        &mut self,
        controls: &StorageRequestContext<'_>,
    ) -> Result<(), CassieError> {
        check_controls(controls)?;
        if self.scan.ranges != [ColumnIdRange::default()] {
            return Ok(());
        }
        let records = super::super::pruning::load_records(&self.tx, self.scan.relation_id)?;
        let mut starts = records
            .into_iter()
            .filter_map(|slot| slot.start)
            .collect::<Vec<_>>();
        starts.sort_unstable();
        starts.dedup();
        if starts.is_empty() {
            return Ok(());
        }
        let _memory = controls.reserve_query_memory(
            starts
                .iter()
                .map(String::len)
                .fold(0, usize::saturating_add),
        )?;
        let mut ranges = Vec::with_capacity(starts.len() + 1);
        let mut previous: Option<String> = None;
        for start in starts {
            ranges.push(ColumnIdRange {
                start: previous.take(),
                end: Some(start.clone()),
            });
            previous = Some(start);
        }
        ranges.push(ColumnIdRange {
            start: previous,
            end: None,
        });
        self.scan.set_ranges(ranges);
        Ok(())
    }

    /// A reader of one range, or `None` past the last range.
    pub(crate) fn partition(
        &self,
        midge: &Midge,
        index: usize,
    ) -> Option<ColumnPartitionReader<'_>> {
        let range = self.scan.ranges.get(index)?.clone();
        midge.column_vector_counters().record_partition();
        Some(ColumnPartitionReader {
            scan: self.scan.for_range(range),
            tx: &self.tx,
        })
    }
}

/// Compile-time proof that workers may share a reader and its partitions.
const fn assert_shareable<T: Sync>() {}
const _: () = assert_shareable::<ColumnVectorReader>();
const fn assert_sendable<T: Send>() {}
const _: () = assert_sendable::<ColumnPartitionReader<'static>>();
const _: () = assert_sendable::<ColumnVectorBatch>();
