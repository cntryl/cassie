//! Test-only ownership change at the actual worker/outer-completion boundary.
use crate::app::{Cassie, CassieSession};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

type Callback = Box<dyn FnOnce(&CassieSession) + Send>;
static NEXT: OnceLock<Mutex<HashMap<usize, Callback>>> = OnceLock::new();

pub(super) struct Guard(usize);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(registry) = NEXT.get() {
            registry.lock().unwrap().remove(&self.0);
        }
    }
}

pub(super) fn install(
    cassie: &Cassie,
    action: impl FnOnce(&CassieSession) + Send + 'static,
) -> Guard {
    let key = Arc::as_ptr(cassie.runtime()) as usize;
    assert!(NEXT
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .insert(key, Box::new(action))
        .is_none());
    Guard(key)
}

pub(super) fn after_worker(cassie: &Cassie, session: &CassieSession) {
    let action = NEXT.get().and_then(|registry| {
        registry
            .lock()
            .unwrap()
            .remove(&(Arc::as_ptr(cassie.runtime()) as usize))
    });
    if let Some(action) = action {
        action(session);
    }
}
