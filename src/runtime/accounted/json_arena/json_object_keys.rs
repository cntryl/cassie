//! Capacity-coupled object-key Value construction from a retained arena.
use super::{Forest, Kind, NodeId};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

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

pub(in crate::runtime::accounted::json_arena) fn build(
    forest: &Forest,
    root: NodeId,
    controls: &QueryExecutionControls,
) -> Result<Option<OwnedObjectKeys>, CassieError> {
    let Kind::Object(index) = &forest.nodes.get(root)?.kind else {
        return Ok(None);
    };
    let empty = controls.reserve_query_memory(0)?;
    let (values, memory) =
        crate::runtime::owned_memory::try_reserve_vec(index.values_len(), &empty)?;
    let mut building = Building { values, memory };
    let mut position = index.first_index()?;
    while let Some(current) = position {
        controls.check_interruption()?;
        let key = index.item(current).0;
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
        building.memory.absorb(text_memory);
        building.values.push(serde_json::Value::String(text));
        visited();
        controls.check_interruption()?;
        position = index.next_index(current, &mut || {})?;
    }
    drop(empty);
    Ok(Some(OwnedObjectKeys {
        value: Value::Json(serde_json::Value::Array(building.values)),
        memory: building.memory,
    }))
}
