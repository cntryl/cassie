//! Parallel waves over the authoritative column vector source (#1358, S5).
//!
//! The persisted id space splits into contiguous ranges (segment boundaries,
//! or the ranges segment pruning kept). A wave hands one range to each worker;
//! a worker owns its range's reader on the statement's snapshot, decodes lane
//! batches, and returns one result. The coordinator joins every worker before
//! it looks at any result and merges results in partition (id) order, so a
//! merged result equals the serial fold over the same rows.
//!
//! Workers share only borrowed state: the reader (and through it the
//! statement's transaction), the schema, the query controls and the runtime.
//! They never open a transaction and never consult a thread-local scope, so
//! they cannot read a different snapshot than the statement's.

use super::{lane_batch, AuthoritativeSource};
use crate::app::Cassie;
use crate::executor::retained_memory::{add, mul};
use crate::executor::scan::conversion;
use crate::executor::typed_batch::TypedBatch;
use crate::executor::QueryError;
use crate::midge::adapter::{ColumnPartitionReader, COLUMN_VECTOR_BATCH_ROWS};
use crate::runtime::{OperatorWorkerGuard, QueryExecutionControls};
use crate::types::{DataType, Value};
use std::mem::size_of;

/// The configured worker limit is one.
pub(crate) const LIMIT_ONE: &str = "column-vector-worker-limit-one";
/// No operator worker permits were available.
pub(crate) const WORKER_BUDGET: &str = "column-vector-worker-budget";
/// The table has one range to read.
pub(crate) const SINGLE_PARTITION: &str = "column-vector-single-partition";
/// The session has staged rows, which merge serially (D5).
pub(crate) const STAGED_SERIAL: &str = "column-vector-staged-serial";

/// How a vector scan runs.
pub(crate) enum Partitioning<'scan> {
    /// Serially, for the stated label.
    Serial(&'static str),
    /// In waves of workers over partitions.
    Parallel(PartitionedScan<'scan>),
}

/// The partitions of a vector scan and the workers that read them.
pub(crate) struct PartitionedScan<'scan> {
    source: &'scan AuthoritativeSource,
    cassie: &'scan Cassie,
    schema: &'scan [(String, DataType)],
    controls: &'scan QueryExecutionControls,
    workers: usize,
    // Released after the last wave, even on failure.
    _guard: OperatorWorkerGuard,
}

/// Counts of one parallel run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WaveStats {
    pub(crate) workers: usize,
    pub(crate) partitions: usize,
}

impl AuthoritativeSource {
    /// Decides between a serial read and waves of up to `requested` workers.
    /// Nothing is read before the decision.
    pub(in crate::executor::scan::typed) fn partitioning<'scan>(
        &'scan mut self,
        cassie: &'scan Cassie,
        schema: &'scan [(String, DataType)],
        requested: usize,
        controls: &'scan QueryExecutionControls,
    ) -> Result<Partitioning<'scan>, QueryError> {
        let serial = |label: &'static str| {
            cassie.midge.column_vector_counters().record_fallback(label);
            Ok(Partitioning::Serial(label))
        };
        if requested <= 1 {
            return serial(LIMIT_ONE);
        }
        if !self.staged.ids().is_empty() {
            return serial(STAGED_SERIAL);
        }
        if self.current.is_some() || self.persisted_done {
            return serial(SINGLE_PARTITION);
        }
        self.reader
            .split_by_segments(controls)
            .map_err(|error| super::controlled_storage_error(cassie, error))?;
        let partitions = self.reader.partition_ranges().len();
        if partitions < 2 {
            return serial(SINGLE_PARTITION);
        }
        let Some(guard) = cassie
            .runtime
            .try_acquire_operator_workers(requested.min(partitions))
        else {
            return serial(WORKER_BUDGET);
        };
        let workers = guard.workers();
        Ok(Partitioning::Parallel(PartitionedScan {
            source: self,
            cassie,
            schema,
            controls,
            workers,
            _guard: guard,
        }))
    }
}

/// The batches of one partition, read by the worker that owns it.
pub(crate) struct PartitionBatches<'wave> {
    reader: ColumnPartitionReader<'wave>,
    scan: &'wave PartitionedScan<'wave>,
}

impl PartitionBatches<'_> {
    /// The next typed batch of the partition.
    ///
    /// # Errors
    ///
    /// Returns storage, resource-limit and cancellation errors.
    pub(crate) fn next(&mut self) -> Result<Option<TypedBatch>, QueryError> {
        let scan = self.scan;
        conversion::check_controls(scan.controls)?;
        let Some(batch) = self
            .reader
            .next_batch(&scan.cassie.midge, COLUMN_VECTOR_BATCH_ROWS, scan.controls)
            .map_err(|error| super::controlled_storage_error(scan.cassie, error))?
        else {
            return Ok(None);
        };
        lane_batch(
            scan.cassie,
            scan.source.reader.outputs(),
            scan.schema,
            batch,
            scan.controls,
        )
        .map(Some)
    }
}

impl PartitionedScan<'_> {
    pub(crate) fn partitions(&self) -> usize {
        self.source.reader.partition_ranges().len()
    }

    /// The bytes a wave reserves before any worker starts: per worker, one
    /// decoded batch of carriers, the partial it may retain and the thread
    /// bookkeeping.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error when the size overflows.
    pub(crate) fn wave_reservation(&self, partial_bytes: usize) -> Result<usize, QueryError> {
        let batch_bytes = mul(
            self.schema.len(),
            mul(COLUMN_VECTOR_BATCH_ROWS, size_of::<Value>())?,
        )?;
        let worker_bytes = add(
            add(batch_bytes, partial_bytes)?,
            size_of::<std::thread::ScopedJoinHandle<'_, ()>>(),
        )?;
        Ok(mul(self.workers, worker_bytes)?)
    }

    /// Runs `work` once per partition, `workers` partitions at a time, and
    /// passes the results to `merge` in partition order.
    ///
    /// `partial_bytes` is what one worker's result may retain beyond its
    /// batches; the wave reserves `workers` times it, plus one decoded batch
    /// and the thread bookkeeping, before any worker starts.
    ///
    /// # Errors
    ///
    /// Joins every worker first, then returns the first failure in partition
    /// order; no result is merged after a failure.
    pub(crate) fn run<T: Send>(
        &self,
        partial_bytes: usize,
        work: impl Fn(&mut PartitionBatches<'_>) -> Result<T, QueryError> + Sync,
        mut merge: impl FnMut(T) -> Result<(), QueryError>,
    ) -> Result<WaveStats, QueryError> {
        let _wave_memory = self
            .controls
            .reserve_query_memory(self.wave_reservation(size_of::<T>().max(partial_bytes))?)?;
        let partitions = self.partitions();
        self.cassie
            .midge
            .column_vector_counters()
            .record_parallel_scan(self.workers);
        let work = &work;
        let mut first = 0;
        while first < partitions {
            conversion::check_controls(self.controls)?;
            let wave = self.workers.min(partitions - first);
            let results = std::thread::scope(|scope| {
                let handles = (first..first + wave)
                    .map(|index| scope.spawn(move || self.read_partition(index, work)))
                    .collect::<Vec<_>>();
                // Join every worker before any failure returns, so every
                // reader and result is released first.
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .map_err(|_| {
                                QueryError::General("column vector worker panicked".to_owned())
                            })
                            .and_then(|result| result)
                    })
                    .collect::<Vec<_>>()
            });
            for result in results {
                merge(result?)?;
            }
            first += wave;
        }
        conversion::check_controls(self.controls)?;
        Ok(WaveStats {
            workers: self.workers,
            partitions,
        })
    }

    fn read_partition<T>(
        &self,
        index: usize,
        work: &(impl Fn(&mut PartitionBatches<'_>) -> Result<T, QueryError> + Sync),
    ) -> Result<T, QueryError> {
        let reader = self
            .source
            .reader
            .partition(&self.cassie.midge, index)
            .ok_or_else(|| QueryError::General("column vector partition missing".to_owned()))?;
        work(&mut PartitionBatches { reader, scan: self })
    }
}

/// Compile-time proof of what workers share across threads.
const fn shared<T: Sync>() {}
const _: () = shared::<AuthoritativeSource>();
const _: () = shared::<Cassie>();
const _: () = shared::<QueryExecutionControls>();
const _: () = shared::<crate::midge::adapter::ColumnVectorReader>();
const _: () = shared::<Vec<(String, DataType)>>();
const fn sent<T: Send>() {}
const _: () = sent::<TypedBatch>();
