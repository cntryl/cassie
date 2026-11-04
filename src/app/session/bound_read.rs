//! Bind-owned command views carry leases, never the Bind request's controls.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::Weak;

use super::{Arc, CassieError, CassieSession, Mutex, SessionTransactionState};
use crate::catalog::ControlledCatalogSnapshot;
use crate::executor::retained_memory::add;
use crate::midge::adapter::{Midge, StatementDataRead};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug)]
pub(crate) struct BoundReadView {
    data: Option<Arc<StatementDataRead>>,
    catalog: Arc<ControlledCatalogSnapshot>,
    identity: Weak<Mutex<SessionTransactionState>>,
    generation: u64,
    midge_identity: usize,
    _memory: QueryMemoryReservation,
    _retention: QueryMemoryReservation,
}

impl BoundReadView {
    pub(crate) fn data(&self) -> Option<&Arc<StatementDataRead>> {
        self.data.as_ref()
    }

    pub(crate) fn catalog(&self) -> Arc<ControlledCatalogSnapshot> {
        Arc::clone(&self.catalog)
    }

    fn matches(&self, session: &CassieSession, midge: &Arc<Midge>) -> bool {
        std::ptr::eq(self.identity.as_ptr(), Arc::as_ptr(&session.transaction))
            && self.generation == session.transaction_generation()
            && session.is_transaction_active()
            && self.midge_identity == Arc::as_ptr(midge) as usize
    }
}

impl CassieSession {
    pub(crate) fn capture_bound_read_view(
        &self,
        midge: &Arc<Midge>,
        database: &str,
        needs_data: bool,
        controls: &QueryExecutionControls,
        retention: &QueryExecutionControls,
    ) -> Result<Option<Arc<BoundReadView>>, CassieError> {
        // A permanently read-only RR owner has no own changes to pin. Saved
        // writable modes are included in this authority predicate before capture;
        // their restoration must not bypass a Bind view or the row fence.
        if !self.is_repeatable_read_write() {
            return Ok(None);
        }
        let Some(initial_catalog) = self.transaction_catalog_snapshot() else {
            return Ok(None);
        };
        controls.check_interruption()?;
        let generation = self.transaction_generation();
        let catalog = self
            .command_catalog_snapshot(controls)?
            .expect("retained catalog");
        let data = needs_data
            .then(|| self.repeatable_statement_read(midge, database, controls))
            .transpose()?
            .flatten();
        let header = add(
            size_of::<BoundReadView>() + 2 * size_of::<usize>(),
            size_of::<Mutex<SessionTransactionState>>() + 2 * size_of::<usize>(),
        )?;
        let additional_data = if self.is_repeatable_read_write() {
            data.as_ref()
                .map_or(Ok(0), |owner| owner.command_accounted_bytes())?
        } else {
            0
        };
        let additional_catalog = if Arc::ptr_eq(&initial_catalog, &catalog) {
            0
        } else {
            catalog.accounted_bytes()
        };
        let memory = controls.reserve_query_memory(header)?;
        // Connection admission bounds the retained view independently of the
        // query that allocated it. Both leases follow the last frame clone.
        let quota = retention
            .reserve_query_memory(add(header, add(additional_data, additional_catalog)?)?)?;
        controls.check_interruption()?;
        if generation != self.transaction_generation() || !self.is_transaction_active() {
            return Err(stale_bound_read());
        }
        Ok(Some(Arc::new(BoundReadView {
            data,
            catalog,
            identity: Arc::downgrade(&self.transaction),
            generation,
            midge_identity: Arc::as_ptr(midge) as usize,
            _memory: memory,
            _retention: quota,
        })))
    }
}

thread_local! {
    static PENDING: RefCell<Option<Arc<BoundReadView>>> = const { RefCell::new(None) };
}

pub(crate) struct BoundReadScope {
    previous: Option<Arc<BoundReadView>>,
    _thread: PhantomData<Rc<()>>,
}

impl BoundReadScope {
    pub(crate) fn enter(owner: Option<Arc<BoundReadView>>) -> Self {
        Self {
            previous: PENDING.with(|slot| slot.replace(owner)),
            _thread: PhantomData,
        }
    }

    /// Only the first actual command adopts a portal view. Nested SQL commands
    /// acquire their own command view; internal CTAS bound phases inherit it.
    pub(crate) fn take(
        session: &CassieSession,
        midge: &Arc<Midge>,
        database: &str,
    ) -> Result<Option<Arc<BoundReadView>>, CassieError> {
        let owner = PENDING.with(|slot| slot.take());
        if let Some(owner) = &owner {
            if !owner.matches(session, midge)
                || owner
                    .data
                    .as_ref()
                    .is_some_and(|data| !data.matches(midge, database))
            {
                return Err(stale_bound_read());
            }
        }
        Ok(owner)
    }
}

impl Drop for BoundReadScope {
    fn drop(&mut self) {
        PENDING.with(|slot| slot.replace(self.previous.take()));
    }
}

fn stale_bound_read() -> CassieError {
    CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message: "bound read view no longer belongs to the active transaction".to_owned(),
    }
}

#[cfg(test)]
mod tests;
