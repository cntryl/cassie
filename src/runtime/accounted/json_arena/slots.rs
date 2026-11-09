//! Private generation-checked value slots; no parser or production caller yet.
use std::sync::atomic::{AtomicU64, Ordering};

use crate::app::CassieError;
use crate::runtime::{accounted::Accounted, QueryExecutionControls, QueryMemoryReservation};

use super::directory::{Directory, Progress as DirectoryProgress};

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;

static NEXT_NAMESPACE: AtomicU64 = AtomicU64::new(1);
const CHUNK_SIZE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NodeId {
    namespace: u64,
    index: usize,
    generation: u64,
}

impl NodeId {
    const fn new(namespace: u64, index: usize, generation: u64) -> Self {
        Self {
            namespace,
            index,
            generation,
        }
    }
    #[cfg(test)]
    const fn index(self) -> usize {
        self.index
    }
    #[cfg(test)]
    const fn generation(self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy)]
enum Progress {
    Move,
    ChunkPublished,
}

struct Slot<T> {
    value: Option<Accounted<T>>,
    generation: u64,
    next_free: Option<usize>,
}

struct Chunk<T> {
    slots: Vec<Option<Slot<T>>>,
    _memory: QueryMemoryReservation,
}

pub(super) struct ValueSlots<T> {
    chunks: Directory<Chunk<T>>,
    controls: QueryExecutionControls,
    namespace: u64,
    length: usize,
    free: Option<usize>,
    poisoned: bool,
}

impl<T> ValueSlots<T> {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let namespace = allocate_namespace(&NEXT_NAMESPACE)?;
        Ok(Self {
            chunks: Directory::new(controls)?,
            controls: controls.clone(),
            namespace,
            length: 0,
            free: None,
            poisoned: false,
        })
    }

    #[cfg(test)]
    fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub(super) fn insert(&mut self, value: Accounted<T>) -> Result<NodeId, CassieError> {
        self.insert_with_observer(value, |_| Ok(()))
    }

    #[cfg(test)]
    fn insert_observed(
        &mut self,
        value: Accounted<T>,
        observer: impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<NodeId, CassieError> {
        self.insert_with_observer(value, observer)
    }

    fn insert_with_observer(
        &mut self,
        value: Accounted<T>,
        mut observer: impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<NodeId, CassieError> {
        self.ensure_usable()?;
        self.controls.check_interruption()?;
        if !value.reservation.shares_tracker(&self.controls) {
            return Err(CassieError::Execution(
                "JSON arena value uses a different query tracker".into(),
            ));
        }
        if let Some(index) = self.free {
            let slot = self.slot_at_mut(index).expect("valid free slot");
            let generation = slot
                .generation
                .checked_add(1)
                .expect("exhausted slots are retired");
            let next = slot.next_free.take();
            slot.generation = generation;
            slot.value = Some(value);
            self.free = next;
            return Ok(NodeId::new(self.namespace, index, generation));
        }
        self.grow(&mut observer)?;
        self.controls.check_interruption()?;
        let index = self.length;
        let next_length = index
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSON arena slot count overflow".into()))?;
        let id = NodeId::new(self.namespace, index, 1);
        let chunk = self
            .chunks
            .get_mut(index / CHUNK_SIZE)
            .expect("allocated chunk");
        chunk.slots[index % CHUNK_SIZE] = Some(Slot {
            value: Some(value),
            generation: 1,
            next_free: None,
        });
        self.length = next_length;
        Ok(id)
    }

    pub(super) fn get(&self, id: NodeId) -> Result<&T, CassieError> {
        self.ensure_usable()?;
        let slot = self.slot(id)?;
        Ok(slot.value.as_ref().expect("live slot").get())
    }

    pub(super) fn get_mut(&mut self, id: NodeId) -> Result<&mut T, CassieError> {
        self.ensure_usable()?;
        if id.namespace != self.namespace {
            return Err(invalid_handle());
        }
        let slot = self
            .slot_at_mut(id.index)
            .filter(|slot| slot.generation == id.generation && slot.value.is_some())
            .ok_or_else(invalid_handle)?;
        Ok(&mut slot.value.as_mut().expect("live slot").value)
    }

    #[cfg(test)]
    pub(super) fn remove(&mut self, id: NodeId) -> Result<Accounted<T>, CassieError> {
        self.ensure_usable()?;
        self.controls.check_interruption()?;
        self.slot(id)?;
        Ok(self.remove_validated(id))
    }

    pub(super) fn remove_validated(&mut self, id: NodeId) -> Accounted<T> {
        debug_assert_eq!(id.namespace, self.namespace);
        if id.namespace != self.namespace {
            unreachable!("validated node namespace");
        }
        let free = self.free;
        let slot = self
            .slot_at_mut(id.index)
            .filter(|slot| slot.generation == id.generation && slot.value.is_some())
            .expect("validated live node");
        let value = slot.value.take().expect("live slot");
        if slot.generation != u64::MAX {
            slot.next_free = free;
            self.free = Some(id.index);
        }
        value
    }

    fn slot(&self, id: NodeId) -> Result<&Slot<T>, CassieError> {
        if id.namespace != self.namespace {
            return Err(invalid_handle());
        }
        self.chunks
            .get(id.index / CHUNK_SIZE)
            .and_then(|chunk| chunk.slots[id.index % CHUNK_SIZE].as_ref())
            .filter(|slot| slot.generation == id.generation && slot.value.is_some())
            .ok_or_else(invalid_handle)
    }

    fn slot_at_mut(&mut self, index: usize) -> Option<&mut Slot<T>> {
        self.chunks.get_mut(index / CHUNK_SIZE)?.slots[index % CHUNK_SIZE].as_mut()
    }

    fn ensure_usable(&self) -> Result<(), CassieError> {
        if self.poisoned {
            Err(CassieError::Execution(
                "JSON arena slots are poisoned".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn grow(
        &mut self,
        observer: &mut impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<(), CassieError> {
        if !self.length.is_multiple_of(CHUNK_SIZE) {
            return Ok(());
        }
        if self.chunks.get(self.length / CHUNK_SIZE).is_some() {
            return Ok(());
        }
        let admission = self.controls.reserve_query_memory(0)?;
        let memory;
        let mut slots;
        (slots, memory) = crate::runtime::owned_memory::try_reserve_vec(CHUNK_SIZE, &admission)?;
        for _ in 0..CHUNK_SIZE {
            self.controls.check_interruption()?;
            slots.push(None);
        }
        self.chunks.push_observed(
            Chunk {
                slots,
                _memory: memory,
            },
            |progress| {
                if matches!(progress, DirectoryProgress::Moved(_)) {
                    observer(Progress::Move)?;
                }
                Ok(())
            },
        )?;
        observer(Progress::ChunkPublished)?;
        self.controls.check_interruption()
    }
}

fn allocate_namespace(counter: &AtomicU64) -> Result<u64, CassieError> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSON arena namespace exhausted".into()))?;
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(previous) => return Ok(previous),
            Err(observed) => current = observed,
        }
    }
}

fn invalid_handle() -> CassieError {
    CassieError::Execution("invalid or stale JSON arena node handle".into())
}

#[cfg(test)]
impl<T> ValueSlots<T> {
    fn test_set_generation(&mut self, id: NodeId, generation: u64) -> NodeId {
        self.slot(id).expect("live");
        self.slot_at_mut(id.index).expect("slot").generation = generation;
        NodeId::new(id.namespace, id.index, generation)
    }
}
