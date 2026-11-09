//! Keep admitted expansion chunk directories alive through their final consumer.
use super::{observe, output::RetainedRows, Progress};
use crate::app::CassieError;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::sync::Arc;

#[cfg(test)]
#[path = "json_object_keys_chunks_tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    static OBSERVE_SOURCE: std::cell::RefCell<Option<(std::sync::Weak<QueryMemoryReservation>, std::rc::Rc<std::cell::Cell<bool>>)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn observe_source() {
    OBSERVE_SOURCE.with(|slot| {
        if let Some((owner, observed)) = slot.borrow_mut().take() {
            observed.set(owner.upgrade().is_some());
        }
    });
}
#[cfg(not(test))]
fn observe_source() {}

#[derive(Debug)]
pub(crate) struct OwnedExpanded {
    batches: Vec<Batch>,
    chunk_memory: Option<QueryMemoryReservation>,
}

impl OwnedExpanded {
    pub(crate) const fn untracked(batches: Vec<Batch>) -> Self {
        Self {
            batches,
            chunk_memory: None,
        }
    }
    pub(crate) fn into_rows(self) -> OwnedExpandedIter {
        let Self {
            batches,
            chunk_memory,
        } = self;
        OwnedExpandedIter {
            current: Vec::new().into_iter(),
            batches: batches.into_iter(),
            _chunk_memory: chunk_memory,
        }
    }
    pub(crate) fn batches(&self) -> &Vec<Batch> {
        &self.batches
    }
    pub(crate) fn batches_mut(&mut self) -> &mut Vec<Batch> {
        &mut self.batches
    }
    pub(crate) fn transform<F>(&mut self, operation: F) -> Result<(), QueryError>
    where
        F: FnOnce(Vec<Batch>) -> Result<Vec<Batch>, QueryError>,
    {
        let batches = std::mem::take(&mut self.batches);
        self.batches = operation(batches)?;
        Ok(())
    }
    pub(crate) fn flatten(
        mut self,
        controls: &QueryExecutionControls,
    ) -> Result<crate::executor::execution::owned_rows::OwnedRows, QueryError> {
        let Some(memory) = self.chunk_memory.as_ref() else {
            return Ok(
                crate::executor::execution::owned_rows::OwnedRows::untracked(
                    batch::flatten_batches(std::mem::take(&mut self.batches)),
                ),
            );
        };
        observe(Progress::Flatten);
        controls.check_interruption()?;
        let count = self.batches.iter().try_fold(0_usize, |count, batch| {
            count.checked_add(batch.len()).ok_or_else(|| {
                CassieError::ResourceLimit("JSON object-key flatten directory overflow".into())
            })
        })?;
        let (rows, memory) = crate::runtime::owned_memory::try_reserve_vec(count, memory)?;
        let mut pending = PendingFlatten { rows, memory };
        for batch in std::mem::take(&mut self.batches) {
            for row in batch {
                controls.check_interruption()?;
                pending.rows.push(row);
                observe(Progress::FlattenMove);
                controls.check_interruption()?;
            }
        }
        let PendingFlatten { rows, memory } = pending;
        let rows = crate::executor::execution::owned_rows::OwnedRows::new(rows, memory, controls)?;
        drop(self);
        Ok(rows)
    }
    #[cfg(test)]
    pub(crate) fn chunk_bytes(&self) -> usize {
        self.chunk_memory
            .as_ref()
            .map_or(0, QueryMemoryReservation::bytes)
    }
}
pub(crate) struct OwnedExpandedIter {
    current: std::vec::IntoIter<BatchRow>,
    batches: std::vec::IntoIter<Batch>,
    _chunk_memory: Option<QueryMemoryReservation>,
}

impl Iterator for OwnedExpandedIter {
    type Item = BatchRow;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(row) = self.current.next() {
                observe(Progress::PullMove);
                return Some(row);
            }
            self.current = self.batches.next()?.into_iter();
        }
    }
}

impl std::ops::Deref for OwnedExpanded {
    type Target = [Batch];
    fn deref(&self) -> &Self::Target {
        &self.batches
    }
}

struct PendingFlatten {
    rows: Vec<BatchRow>,
    memory: QueryMemoryReservation,
}

struct PendingInput {
    rows: Option<Vec<BatchRow>>,
    owner: Option<Arc<QueryMemoryReservation>>,
}
struct Builder {
    input: std::vec::IntoIter<BatchRow>,
    batches: Vec<Batch>,
    current: Batch,
    memory: QueryMemoryReservation,
    source_owner: Option<Arc<QueryMemoryReservation>>,
}

pub(super) fn build(
    retained: RetainedRows,
    controls: &QueryExecutionControls,
) -> Result<OwnedExpanded, CassieError> {
    let RetainedRows { rows, source_owner } = retained;
    let mut pending = PendingInput {
        rows: Some(rows),
        owner: source_owner,
    };
    if pending.rows.as_ref().expect("source rows").is_empty() {
        return Ok(OwnedExpanded::untracked(Vec::new()));
    }
    if !pending
        .owner
        .as_ref()
        .is_some_and(|owner| owner.belongs_to(controls))
    {
        return Err(CassieError::ResourceLimit(
            "JSON object-key source uses a different query tracker".into(),
        ));
    }
    let memory = controls.reserve_query_memory(0)?;
    let input = pending.rows.take().expect("source rows").into_iter();
    let mut builder = Builder {
        input,
        batches: Vec::new(),
        current: Vec::new(),
        memory,
        source_owner: pending.owner.take(),
    };
    let capacity = builder.input.len().div_ceil(batch::DEFAULT_BATCH_SIZE);
    let (batches, memory) =
        crate::runtime::owned_memory::try_reserve_vec(capacity, &builder.memory)?;
    builder.batches = batches;
    builder.memory.absorb(memory);
    observe(Progress::ChunkDirectory);
    controls.check_interruption()?;
    while builder.input.len() > 0 {
        let count = builder.input.len().min(batch::DEFAULT_BATCH_SIZE);
        let (current, memory) =
            crate::runtime::owned_memory::try_reserve_vec(count, &builder.memory)?;
        builder.current = current;
        builder.memory.absorb(memory);
        for _ in 0..count {
            controls.check_interruption()?;
            let row = builder.input.next().expect("counted source row");
            builder.current.push(row);
            observe(Progress::ChunkMove);
            observe_source();
            controls.check_interruption()?;
        }
        builder.batches.push(std::mem::take(&mut builder.current));
        batch::record_batch_buffer();
    }
    let Builder {
        input,
        batches,
        current,
        memory,
        source_owner,
    } = builder;
    drop(input);
    drop(current);
    drop(source_owner);
    Ok(OwnedExpanded {
        batches,
        chunk_memory: Some(memory),
    })
}
