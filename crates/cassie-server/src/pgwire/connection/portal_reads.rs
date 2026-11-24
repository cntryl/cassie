//! Connection quota and lifetime ownership for private Bind read views.

use std::mem::size_of;
use std::sync::Arc;

use crate::app::{BoundReadView, CassieError};
use crate::runtime::{owned_memory, QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug)]
struct PortalReadOwner {
    name: String,
    view: Arc<BoundReadView>,
    _memory: QueryMemoryReservation,
}

#[derive(Debug, Default)]
pub(super) struct PortalReadOwners {
    owners: Vec<PortalReadOwner>,
    directory: Option<QueryMemoryReservation>,
}

impl PortalReadOwners {
    pub(super) fn get(&self, name: &str) -> Option<Arc<BoundReadView>> {
        self.owners
            .iter()
            .find(|owner| owner.name == name)
            .map(|owner| Arc::clone(&owner.view))
    }

    pub(super) fn insert(
        &mut self,
        name: &str,
        view: Arc<BoundReadView>,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if let Some(owner) = self.owners.iter_mut().find(|owner| owner.name == name) {
            owner.view = view;
            return Ok(());
        }
        let memory = controls.reserve_query_memory(name.len())?;
        let mut owned_name = String::new();
        owned_name
            .try_reserve_exact(name.len())
            .map_err(|error| owned_memory::allocation_error(&error))?;
        if owned_name.capacity() > name.len() {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted portal read name".to_owned(),
            ));
        }
        owned_name.push_str(name);
        if self.directory.is_none() {
            self.directory = Some(controls.reserve_query_memory(0)?);
        }
        let directory = self.directory.as_mut().expect("initialized above");
        owned_memory::grow_vec(&mut self.owners, 1, directory)?;
        debug_assert!(directory.bytes() >= self.owners.capacity() * size_of::<PortalReadOwner>());
        self.owners.push(PortalReadOwner {
            name: owned_name,
            view,
            _memory: memory,
        });
        Ok(())
    }

    pub(super) fn remove(&mut self, name: &str) {
        if let Some(index) = self.owners.iter().position(|owner| owner.name == name) {
            self.owners.swap_remove(index);
        }
        if self.owners.is_empty() {
            self.owners = Vec::new();
            self.directory = None;
        }
    }
}

#[cfg(test)]
mod tests;
