//! Bounded streaming waves; each worker owns one contiguous input batch.
use super::{consume, Spec, State};
use crate::app::Cassie;
use crate::executor::retained_memory::{add, mul};
use crate::executor::scan::TypedScanStream;
use crate::executor::typed_batch::{check_controls, TypedBatch};
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::mem::size_of;

pub(super) struct Diagnostics {
    pub(super) workers: usize,
    pub(super) partitions: usize,
    pub(super) rows: usize,
    pub(super) fallback: &'static str,
}
struct Partial {
    states: Vec<State>,
    _memory: QueryMemoryReservation,
}

pub(super) fn consume_stream(
    cassie: &Cassie,
    stream: &mut TypedScanStream<'_>,
    specs: &mut [Spec],
    predicate: Option<&crate::sql::ast::Expr>,
    controls: &QueryExecutionControls,
) -> Result<Diagnostics, QueryError> {
    let requested = cassie.runtime.limits().parallel_aggregation_workers.max(1);
    let legal = specs.iter().all(|spec| spec.merge_safe);
    let guard = (legal && requested > 1)
        .then(|| cassie.runtime.try_acquire_operator_workers(requested))
        .flatten();
    let workers = match &guard {
        Some(guard) => guard.workers(),
        None => 1,
    };
    let fallback = if !legal {
        "typed-row-order-fold"
    } else if requested == 1 {
        "typed-worker-limit-one"
    } else if guard.is_none() {
        "typed-worker-budget"
    } else {
        "typed-small-input"
    };
    let bytes = mul(
        workers,
        add(
            size_of::<TypedBatch>(),
            add(
                size_of::<std::thread::ScopedJoinHandle<'_, Result<Partial, QueryError>>>(),
                size_of::<Result<Partial, QueryError>>(),
            )?,
        )?,
    )?;
    let _wave_memory = controls.reserve_query_memory(bytes)?;
    let mut diagnostic = Diagnostics {
        workers: 1,
        partitions: 0,
        rows: 0,
        fallback,
    };
    loop {
        let mut wave = Vec::with_capacity(workers);
        for _ in 0..workers {
            if let Some(batch) =
                stream.next_batch_bounded(crate::executor::batch::DEFAULT_BATCH_SIZE)?
            {
                let batch = super::predicate::apply(batch, predicate, controls)?;
                diagnostic.rows = diagnostic.rows.checked_add(batch.len()).ok_or_else(|| {
                    QueryError::General("typed aggregate row count overflow".to_owned())
                })?;
                wave.push(batch);
            } else {
                break;
            }
        }
        if wave.is_empty() {
            break;
        }
        diagnostic.partitions += wave.len();
        if wave.len() == 1 {
            for spec in &mut *specs {
                consume(&mut spec.state, spec.column, &wave[0], controls)?;
            }
        } else {
            diagnostic.workers = diagnostic.workers.max(wave.len());
            merge_wave(specs, wave, controls)?;
        }
        #[cfg(test)]
        cancel_after_completed_wave();
    }
    check_controls(controls)?;
    Ok(diagnostic)
}

fn merge_wave(
    specs: &mut [Spec],
    wave: Vec<TypedBatch>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let partials = std::thread::scope(|scope| {
        let specs = &*specs;
        let handles = wave
            .into_iter()
            .map(|batch| scope.spawn(move || fold_partition(specs, batch, controls)))
            .collect::<Vec<_>>();
        // Join every worker before returning any failure, so all source/partial leases end.
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| QueryError::General("typed aggregate worker panicked".to_owned()))
                    .and_then(|result| result)
            })
            .collect::<Vec<_>>()
    });
    for partial in partials {
        let partial = partial?;
        for (spec, later) in specs.iter_mut().zip(&partial.states) {
            check_controls(controls)?;
            spec.state.merge_ordered(later)?;
        }
    }
    Ok(())
}
fn fold_partition(
    specs: &[Spec],
    batch: TypedBatch,
    controls: &QueryExecutionControls,
) -> Result<Partial, QueryError> {
    let memory = controls.reserve_query_memory(mul(specs.len(), size_of::<State>())?)?;
    let mut states = specs
        .iter()
        .map(|spec| spec.state.empty_partial())
        .collect::<Vec<_>>();
    for (state, spec) in states.iter_mut().zip(specs) {
        consume(state, spec.column, &batch, controls)?;
    }
    // The worker owns its source batch until every local fold completes.
    // Release that owner before handing the partial to the parent; errors drop it via RAII.
    drop(batch);
    Ok(Partial {
        states,
        _memory: memory,
    })
}

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_WAVE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) struct WaveCancellationProbe;
#[cfg(test)]
impl WaveCancellationProbe {
    pub(super) fn arm(handle: crate::runtime::QueryCancellationHandle) -> Self {
        CANCEL_AFTER_WAVE.with(|slot| {
            assert!(slot.borrow().is_none(), "wave probes must not nest");
            slot.replace(Some(handle));
        });
        Self
    }
}
#[cfg(test)]
impl Drop for WaveCancellationProbe {
    fn drop(&mut self) {
        CANCEL_AFTER_WAVE.with(|slot| slot.borrow_mut().take());
    }
}
#[cfg(test)]
fn cancel_after_completed_wave() {
    if let Some(handle) = CANCEL_AFTER_WAVE.with(|slot| slot.borrow_mut().take()) {
        handle.cancel();
    }
}
