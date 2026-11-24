//! Typed projected reads over parallel column-vector waves (#1358, S5).
//!
//! Workers decode lane batches and apply a natively typed filter on their own
//! partition. The coordinator receives partitions in id order, evaluates any
//! filter the workers did not (expressions outside the native subset run on
//! the statement's thread), projects, and emits rows, so output order is the
//! serial row-id order.
use super::{
    output_rows, BatchRow, Cassie, CassieSession, FunctionMeta, HashMap, LogicalPlan, QueryError,
    QueryExecutionControls, Value,
};
use crate::executor::retained_memory::mul;
use crate::executor::scan::PartitionedScan;
use crate::executor::typed_batch::capability::{self, Capability};
use crate::executor::typed_batch::TypedBatch;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What one partition's worker hands back.
struct Partition {
    batches: Vec<TypedBatch>,
    /// The workers applied the plan's filter to `batches`.
    filtered: bool,
}

pub(super) struct Collected {
    pub(super) rows: Vec<BatchRow>,
    pub(super) scanned_rows: usize,
}

/// Reads every partition of `scan` and projects the rows in id order.
pub(super) fn collect(
    cassie: &Cassie,
    scan: &PartitionedScan<'_>,
    plan: &LogicalPlan,
    request: (
        &HashMap<String, FunctionMeta>,
        &[Value],
        Option<&CassieSession>,
    ),
    controls: &QueryExecutionControls,
) -> Result<Collected, QueryError> {
    let (functions, params, session) = request;
    let scanned = AtomicUsize::new(0);
    let mut output_memory = controls.reserve_query_memory(0)?;
    let mut rows: Vec<BatchRow> = Vec::new();
    let stats = scan.run(
        0,
        |batches| {
            let mut partition = Partition {
                batches: Vec::new(),
                filtered: plan.filter.is_none(),
            };
            while let Some(batch) = batches.next()? {
                scanned.fetch_add(batch.len(), Ordering::Relaxed);
                let batch = match &plan.filter {
                    Some(predicate)
                        if capability::expression(predicate, batch.schema(), params)
                            == Capability::NativeTyped =>
                    {
                        partition.filtered = true;
                        batch.filter(controls, predicate, params, functions, session)?
                    }
                    _ => batch,
                };
                if batch.len() > 0 {
                    partition.batches.try_reserve(1).map_err(|error| {
                        QueryError::General(format!("unable to retain filtered batches: {error}"))
                    })?;
                    partition.batches.push(batch);
                }
            }
            Ok(partition)
        },
        |partition| {
            for mut batch in partition.batches {
                if !partition.filtered {
                    if let Some(predicate) = &plan.filter {
                        batch = batch.filter(controls, predicate, params, functions, session)?;
                    }
                }
                if batch.len() == 0 {
                    continue;
                }
                let projected =
                    batch.project(controls, &plan.projection, params, functions, session)?;
                if rows.len().saturating_add(projected.len()) > controls.max_result_rows {
                    return Err(crate::app::CassieError::ResourceLimit(
                        "query result row limit exceeded".to_owned(),
                    )
                    .into());
                }
                output_memory.try_grow(mul(projected.len(), size_of::<BatchRow>())?)?;
                rows.reserve_exact(projected.len());
                rows.extend(output_rows(&projected, controls)?);
            }
            Ok(())
        },
    )?;
    cassie.runtime.record_parallel_scan(
        stats.workers,
        stats.partitions,
        scanned.load(Ordering::Relaxed),
    );
    Ok(Collected {
        rows,
        scanned_rows: scanned.load(Ordering::Relaxed),
    })
}
