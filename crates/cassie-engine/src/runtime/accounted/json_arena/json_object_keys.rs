//! Capacity-coupled object-key Value construction from a retained arena.
use super::{Forest, Kind, NodeId};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

#[path = "json_object_keys_sort.rs"]
mod sort;
#[cfg(test)]
pub(crate) use sort::{cancel_object_keys_sort_compare_once, cancel_object_keys_sort_move_once};

#[cfg(test)]
thread_local! {
    static CANCEL_COPY: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_VISIT: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) fn cancel_object_keys_copy_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_COPY.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
pub(crate) fn cancel_object_keys_visit_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_VISIT.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
fn copied() {
    CANCEL_COPY.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn copied() {}
#[cfg(test)]
fn visited() {
    CANCEL_VISIT.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn visited() {}

pub(crate) struct OwnedObjectKeys {
    value: Value,
    memory: QueryMemoryReservation,
}
impl OwnedObjectKeys {
    pub(crate) fn into_parts(self) -> (Value, QueryMemoryReservation) {
        (self.value, self.memory)
    }
}
struct Building {
    values: Vec<serde_json::Value>,
    memory: QueryMemoryReservation,
}

impl Building {
    fn new(length: usize, controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        let empty = controls.reserve_query_memory(0)?;
        let (values, memory) = crate::runtime::owned_memory::try_reserve_vec(length, &empty)?;
        drop(empty);
        Ok(Self { values, memory })
    }
    fn push(&mut self, key: &str, controls: &QueryExecutionControls) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let text_memory = controls.reserve_query_memory(key.len())?;
        let mut text = String::new();
        text.try_reserve_exact(key.len())
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if text.capacity() > key.len() {
            return Err(CassieError::ResourceLimit(
                "JSON object key exceeded its admission".into(),
            ));
        }
        let mut start = 0;
        while start < key.len() {
            controls.check_interruption()?;
            let mut end = start.saturating_add(1024).min(key.len());
            while !key.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&key[start..end]);
            start = end;
            copied();
            controls.check_interruption()?;
        }
        self.memory.absorb(text_memory);
        self.values.push(serde_json::Value::String(text));
        visited();
        controls.check_interruption()?;
        Ok(())
    }
    fn finish(self) -> OwnedObjectKeys {
        OwnedObjectKeys {
            value: Value::Json(serde_json::Value::Array(self.values)),
            memory: self.memory,
        }
    }
}

pub(in crate::runtime::accounted::json_arena) fn build(
    forest: &Forest,
    root: NodeId,
    controls: &QueryExecutionControls,
) -> Result<Option<OwnedObjectKeys>, CassieError> {
    let Kind::Object(index) = &forest.nodes.get(root)?.kind else {
        return Ok(None);
    };
    let mut building = Building::new(index.values_len(), controls)?;
    let mut position = index.first_index()?;
    while let Some(current) = position {
        building.push(index.item(current).0, controls)?;
        position = index.next_index(current, &mut || {})?;
    }
    Ok(Some(building.finish()))
}

pub(crate) fn build_object_keys_from_map(
    map: &serde_json::Map<String, serde_json::Value>,
    controls: &QueryExecutionControls,
) -> Result<OwnedObjectKeys, CassieError> {
    let sorted = sort::keys(map, controls)?;
    let mut building = Building::new(sorted.values.len(), controls)?;
    for key in &sorted.values {
        building.push(key, controls)?;
    }
    Ok(building.finish())
}
