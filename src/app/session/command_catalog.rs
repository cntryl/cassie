//! A command retains its SQL metadata view across internal CTAS phases.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Weak;

use super::{Arc, CassieSession, Mutex, SessionTransactionState};
use crate::catalog::ControlledCatalogSnapshot;

#[derive(Clone)]
struct CommandCatalog {
    owner: Arc<ControlledCatalogSnapshot>,
    identity: Weak<Mutex<SessionTransactionState>>,
    generation: u64,
    repeatable_identity: Option<usize>,
}

thread_local! {
    static CURRENT: RefCell<Option<CommandCatalog>> = const { RefCell::new(None) };
}

pub(crate) struct CommandCatalogScope {
    previous: Option<CommandCatalog>,
    _thread: PhantomData<Rc<()>>,
}

impl CommandCatalogScope {
    pub(crate) fn enter(
        session: &CassieSession,
        owner: Option<Arc<ControlledCatalogSnapshot>>,
    ) -> Self {
        let current = owner.map(|owner| CommandCatalog {
            owner,
            identity: Arc::downgrade(&session.transaction),
            generation: session.transaction_generation(),
            repeatable_identity: session.repeatable_read_identity(),
        });
        Self {
            previous: CURRENT.with(|slot| slot.replace(current)),
            _thread: PhantomData,
        }
    }
}

impl Drop for CommandCatalogScope {
    fn drop(&mut self) {
        CURRENT.with(|slot| slot.replace(self.previous.take()));
    }
}

impl CassieSession {
    pub(crate) fn current_command_catalog(&self) -> Option<Arc<ControlledCatalogSnapshot>> {
        CURRENT.with(|slot| {
            slot.borrow()
                .as_ref()
                .filter(|scope| {
                    (std::ptr::eq(scope.identity.as_ptr(), Arc::as_ptr(&self.transaction))
                        || (scope.repeatable_identity.is_some()
                            && scope.repeatable_identity == self.repeatable_read_identity()))
                        && scope.generation == self.transaction_generation()
                })
                .map(|scope| Arc::clone(&scope.owner))
        })
    }
}
