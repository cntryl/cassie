use super::{Arc, AtomicI32, HashMap, Mutex, Ordering, QueryCancellationHandle, RuntimeState};

#[derive(Debug)]
pub(super) struct PgwireBackendRegistry {
    next_process_id: AtomicI32,
    entries: Mutex<HashMap<i32, RegisteredBackend>>,
}

#[derive(Debug, Clone)]
struct RegisteredBackend {
    secret_key: i32,
    cancellation: Option<QueryCancellationHandle>,
}

impl Default for PgwireBackendRegistry {
    fn default() -> Self {
        Self {
            next_process_id: AtomicI32::new(1_000),
            entries: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PgwireBackendRegistration {
    runtime: Arc<RuntimeState>,
    process_id: i32,
    secret_key: i32,
}

impl PgwireBackendRegistration {
    #[must_use]
    pub(crate) const fn process_id(&self) -> i32 {
        self.process_id
    }

    #[must_use]
    pub(crate) const fn secret_key(&self) -> i32 {
        self.secret_key
    }

    /// # Panics
    ///
    /// Panics if the pgwire backend registry is poisoned.
    pub(crate) fn begin_query(&self) -> PgwireQueryCancellationGuard {
        self.begin_query_with_handle(QueryCancellationHandle::new())
    }

    /// # Panics
    ///
    /// Panics if the pgwire backend registry is poisoned.
    pub(crate) fn resume_query(
        &self,
        cancellation: QueryCancellationHandle,
    ) -> PgwireQueryCancellationGuard {
        self.begin_query_with_handle(cancellation)
    }

    pub(crate) fn clear_query(&self, cancellation: &QueryCancellationHandle) {
        if let Some(backend) = self
            .runtime
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .get_mut(&self.process_id)
        {
            if backend
                .cancellation
                .as_ref()
                .is_some_and(|active| active.is_same_request(cancellation))
            {
                backend.cancellation = None;
            }
        }
    }

    fn begin_query_with_handle(
        &self,
        cancellation: QueryCancellationHandle,
    ) -> PgwireQueryCancellationGuard {
        if let Some(backend) = self
            .runtime
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .get_mut(&self.process_id)
        {
            backend.cancellation = Some(cancellation.clone());
        }
        PgwireQueryCancellationGuard {
            runtime: Arc::clone(&self.runtime),
            process_id: self.process_id,
            cancellation,
            clear_on_drop: true,
        }
    }
}

pub(crate) struct PgwireQueryCancellationGuard {
    runtime: Arc<RuntimeState>,
    process_id: i32,
    cancellation: QueryCancellationHandle,
    clear_on_drop: bool,
}

impl PgwireQueryCancellationGuard {
    #[must_use]
    pub(crate) fn handle(&self) -> QueryCancellationHandle {
        self.cancellation.clone()
    }

    pub(crate) fn suspend(mut self) -> QueryCancellationHandle {
        self.clear_on_drop = false;
        self.cancellation.clone()
    }
}

impl Drop for PgwireQueryCancellationGuard {
    fn drop(&mut self) {
        if !self.clear_on_drop {
            return;
        }
        if let Some(backend) = self
            .runtime
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .get_mut(&self.process_id)
        {
            backend.cancellation = None;
        }
    }
}

impl Drop for PgwireBackendRegistration {
    fn drop(&mut self) {
        self.runtime.unregister_pgwire_backend(self.process_id);
    }
}

pub struct PgwireSessionGuard {
    runtime: Arc<RuntimeState>,
}

impl Drop for PgwireSessionGuard {
    fn drop(&mut self) {
        self.runtime.finish_pgwire_session();
    }
}

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if the pgwire backend registry is poisoned.
    pub(crate) fn register_pgwire_backend(self: &Arc<Self>) -> PgwireBackendRegistration {
        let process_id = self
            .pgwire_backends
            .next_process_id
            .fetch_add(1, Ordering::Relaxed);
        let secret_key = pgwire_secret_key();
        self.pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .insert(
                process_id,
                RegisteredBackend {
                    secret_key,
                    cancellation: None,
                },
            );
        PgwireBackendRegistration {
            runtime: Arc::clone(self),
            process_id,
            secret_key,
        }
    }

    /// # Panics
    ///
    /// Panics if the pgwire backend registry is poisoned.
    pub(crate) fn cancel_pgwire_backend(&self, process_id: i32, secret_key: i32) -> bool {
        let entries = self
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry");
        let Some(backend) = entries.get(&process_id) else {
            return false;
        };
        if backend.secret_key != secret_key {
            return false;
        }
        let Some(cancellation) = backend.cancellation.as_ref() else {
            return false;
        };
        cancellation.cancel();
        true
    }

    fn unregister_pgwire_backend(&self, process_id: i32) {
        self.pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .remove(&process_id);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn begin_pgwire_session(self: &Arc<Self>) -> PgwireSessionGuard {
        {
            let mut metrics = self.metrics.lock().expect("runtime metrics");
            metrics.pgwire.active_sessions += 1;
            metrics.pgwire.sessions_started_total += 1;
        }

        PgwireSessionGuard {
            runtime: Arc::clone(self),
        }
    }

    fn finish_pgwire_session(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.active_sessions = metrics.pgwire.active_sessions.saturating_sub(1);
        metrics.pgwire.sessions_finished_total += 1;
    }
}

fn pgwire_secret_key() -> i32 {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let secret = i32::from_be_bytes(bytes[..4].try_into().expect("uuid prefix"));
    if secret == 0 {
        1
    } else {
        secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;

    #[test]
    fn should_ignore_wrong_pgwire_cancel_secret() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let registration = runtime.register_pgwire_backend();
        let query = registration.begin_query();
        let cancellation = query.handle();
        let wrong_secret = registration.secret_key().wrapping_add(1);

        // Act
        let cancelled = runtime.cancel_pgwire_backend(registration.process_id(), wrong_secret);

        // Assert
        assert!(!cancelled);
        assert!(!cancellation.is_cancelled());
    }
}
