//! Typed aggregates over parallel column-vector waves (#1358, S5).
//!
//! Each worker owns one id range of the table: it decodes lane batches,
//! applies the filter and either folds a partial (COUNT, integer SUM, MIN, MAX
//! merge exactly) or, when an aggregate must see rows in order (float SUM and
//! AVG), hands back its filtered batches. The coordinator merges partitions in
//! id order, so every result equals the serial fold over the same rows.
use super::grouped::{Groups, Shape};
use super::workers::Diagnostics;
use super::{consume, predicate, Spec, State};
use crate::executor::retained_memory::mul;
use crate::executor::scan::PartitionedScan;
use crate::executor::typed_batch::{check_controls, TypedBatch};
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::Expr;
use crate::types::Value;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A partition's states and the admission that covers them.
struct Partial {
    states: Vec<State>,
    _memory: QueryMemoryReservation,
}

/// Runs the ungrouped aggregate over `scan`.
pub(super) fn consume_ungrouped(
    scan: &PartitionedScan<'_>,
    specs: &mut [Spec],
    predicate: Option<&Expr>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Diagnostics, QueryError> {
    let rows = AtomicUsize::new(0);
    let partial_bytes = mul(specs.len(), size_of::<State>())?;
    let stats = if specs.iter().all(|spec| spec.merge_safe) {
        let empties = specs
            .iter()
            .map(|spec| spec.state.empty_partial())
            .collect::<Vec<_>>();
        let columns = specs.iter().map(|spec| spec.column).collect::<Vec<_>>();
        scan.run(
            partial_bytes,
            |batches| {
                let memory = controls.reserve_query_memory(partial_bytes)?;
                let mut states = empties.iter().map(State::empty_partial).collect::<Vec<_>>();
                while let Some(batch) = batches.next()? {
                    let batch = predicate::apply(batch, predicate, params, controls)?;
                    rows.fetch_add(batch.len(), Ordering::Relaxed);
                    for (state, column) in states.iter_mut().zip(&columns) {
                        consume(state, *column, &batch, controls)?;
                    }
                }
                Ok(Partial {
                    states,
                    _memory: memory,
                })
            },
            |partial| {
                for (spec, later) in specs.iter_mut().zip(&partial.states) {
                    check_controls(controls)?;
                    spec.state.merge_ordered(later)?;
                }
                #[cfg(test)]
                super::workers::cancel_after_completed_wave();
                Ok(())
            },
        )?
    } else {
        scan.run(
            0,
            |batches| filtered_batches(batches, predicate, params, controls, &rows),
            |filtered| {
                for batch in &filtered {
                    for spec in &mut *specs {
                        consume(&mut spec.state, spec.column, batch, controls)?;
                    }
                }
                Ok(())
            },
        )?
    };
    Ok(Diagnostics {
        workers: stats.workers,
        partitions: stats.partitions,
        rows: rows.load(Ordering::Relaxed),
        fallback: "",
    })
}

/// Runs the grouped aggregate over `scan`; returns the filtered row count.
pub(super) fn consume_grouped(
    scan: &PartitionedScan<'_>,
    shape: &Shape,
    groups: &mut Groups,
    predicate: Option<&Expr>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Diagnostics, QueryError> {
    let rows = AtomicUsize::new(0);
    let stats = if shape.merge_safe() {
        scan.run(
            shape.group_fixed_bytes()?,
            |batches| {
                let mut local = Groups::new(controls)?;
                while let Some(batch) = batches.next()? {
                    let batch = predicate::apply(batch, predicate, params, controls)?;
                    rows.fetch_add(batch.len(), Ordering::Relaxed);
                    shape.fold(&batch, &mut local, controls)?;
                }
                Ok(local)
            },
            |local| shape.merge(groups, local, controls),
        )?
    } else {
        scan.run(
            0,
            |batches| filtered_batches(batches, predicate, params, controls, &rows),
            |filtered| {
                for batch in &filtered {
                    shape.fold(batch, groups, controls)?;
                }
                Ok(())
            },
        )?
    };
    Ok(Diagnostics {
        workers: stats.workers,
        partitions: stats.partitions,
        rows: rows.load(Ordering::Relaxed),
        fallback: "",
    })
}

/// A partition's filtered batches, for aggregates the coordinator folds in
/// row order.
fn filtered_batches(
    batches: &mut crate::executor::scan::PartitionBatches<'_>,
    predicate: Option<&Expr>,
    params: &[Value],
    controls: &QueryExecutionControls,
    rows: &AtomicUsize,
) -> Result<Vec<TypedBatch>, QueryError> {
    let mut filtered = Vec::new();
    while let Some(batch) = batches.next()? {
        let batch = predicate::apply(batch, predicate, params, controls)?;
        rows.fetch_add(batch.len(), Ordering::Relaxed);
        filtered.try_reserve(1).map_err(|error| {
            QueryError::General(format!("unable to retain filtered batches: {error}"))
        })?;
        filtered.push(batch);
    }
    Ok(filtered)
}
