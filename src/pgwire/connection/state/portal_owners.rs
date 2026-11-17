//! Accounted transaction-lifetime ownership for connection-local wire portals.

use crate::app::{CassieError, FailureAuthorityIdentity};
use crate::runtime::{owned_memory, QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug)]
struct PortalOwner {
    name: String,
    identity: FailureAuthorityIdentity,
    _name_memory: QueryMemoryReservation,
}

#[derive(Debug)]
pub(in crate::pgwire::connection) struct PreparedPortalOwner(PortalOwner);

#[derive(Debug, Default)]
pub(in crate::pgwire::connection) struct PortalOwners {
    owners: Vec<PortalOwner>,
    directory: Option<QueryMemoryReservation>,
}

fn same_lifetime(left: FailureAuthorityIdentity, right: FailureAuthorityIdentity) -> bool {
    left.transaction == right.transaction && left.generation == right.generation
}

impl PortalOwners {
    pub(in crate::pgwire::connection) fn prepare(
        &mut self,
        name: &str,
        identity: FailureAuthorityIdentity,
        controls: &QueryExecutionControls,
    ) -> Result<PreparedPortalOwner, CassieError> {
        let result = self.prepare_inner(name, identity, controls);
        if result.is_err() {
            self.release_empty();
        }
        result
    }

    fn prepare_inner(
        &mut self,
        name: &str,
        identity: FailureAuthorityIdentity,
        controls: &QueryExecutionControls,
    ) -> Result<PreparedPortalOwner, CassieError> {
        let name_memory = controls.reserve_query_memory(name.len())?;
        let mut owned_name = String::new();
        owned_name
            .try_reserve_exact(name.len())
            .map_err(|error| owned_memory::allocation_error(&error))?;
        if owned_name.capacity() > name.len() {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted portal owner name".to_owned(),
            ));
        }
        owned_name.push_str(name);
        if !self.owners.iter().any(|owner| owner.name == name) {
            if self.directory.is_none() {
                self.directory = Some(controls.reserve_query_memory(0)?);
            }
            owned_memory::grow_vec(
                &mut self.owners,
                1,
                self.directory.as_mut().expect("owner directory"),
            )?;
        }
        Ok(PreparedPortalOwner(PortalOwner {
            name: owned_name,
            identity,
            _name_memory: name_memory,
        }))
    }

    pub(in crate::pgwire::connection) fn publish(&mut self, prepared: PreparedPortalOwner) {
        let PreparedPortalOwner(owner) = prepared;
        if let Some(previous) = self
            .owners
            .iter_mut()
            .find(|previous| previous.name == owner.name)
        {
            *previous = owner;
        } else {
            debug_assert!(self.owners.len() < self.owners.capacity());
            self.owners.push(owner);
        }
    }

    pub(in crate::pgwire::connection) fn belongs_to(
        &self,
        name: &str,
        identity: FailureAuthorityIdentity,
    ) -> bool {
        self.owners
            .iter()
            .any(|owner| owner.name == name && same_lifetime(owner.identity, identity))
    }

    pub(in crate::pgwire::connection) fn identity(
        &self,
        name: &str,
    ) -> Option<FailureAuthorityIdentity> {
        self.owners
            .iter()
            .find(|owner| owner.name == name)
            .map(|owner| owner.identity)
    }

    pub(in crate::pgwire::connection) fn adopt(
        &mut self,
        before: FailureAuthorityIdentity,
        after: FailureAuthorityIdentity,
    ) {
        if before.transaction.is_some() || after.transaction.is_none() {
            return;
        }
        for owner in &mut self.owners {
            if same_lifetime(owner.identity, before) {
                owner.identity = after;
            }
        }
    }

    pub(in crate::pgwire::connection) fn remove(&mut self, name: &str) {
        if let Some(index) = self.owners.iter().position(|owner| owner.name == name) {
            self.owners.swap_remove(index);
        }
        self.release_empty();
    }

    pub(in crate::pgwire::connection) fn release_empty(&mut self) {
        if self.owners.is_empty() {
            self.owners = Vec::new();
            self.directory = None;
        }
    }
}

#[cfg(test)]
mod tests;
