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
    aggregate_pause: Option<Arc<super::aggregate_test_pause::QueryAggregateTestPause>>,
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
        self.begin_query_with_handle(QueryCancellationHandle::new(), true)
    }

    /// # Panics
    ///
    /// Panics if the pgwire backend registry is poisoned.
    pub(crate) fn resume_query(
        &self,
        cancellation: QueryCancellationHandle,
    ) -> PgwireQueryCancellationGuard {
        self.begin_query_with_handle(cancellation, false)
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
        mut cancellation: QueryCancellationHandle,
        claim_pause: bool,
    ) -> PgwireQueryCancellationGuard {
        if let Some(backend) = self
            .runtime
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .get_mut(&self.process_id)
        {
            if claim_pause
                && backend.aggregate_pause.as_ref().is_some_and(|pause| {
                    cancellation.attach_aggregate_test_pause(Arc::clone(pause))
                })
            {
                backend.aggregate_pause = None;
            }
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
                    aggregate_pause: None,
                },
            );
        PgwireBackendRegistration {
            runtime: Arc::clone(self),
            process_id,
            secret_key,
        }
    }

    pub(super) fn pause_backend_after_aggregate_input(
        self: &Arc<Self>,
        process_id: i32,
        secret_key: i32,
    ) -> Result<
        (
            super::QueryAggregateTestPauseGuard,
            std::sync::mpsc::Receiver<()>,
            std::sync::mpsc::SyncSender<()>,
        ),
        super::CassieError,
    > {
        let mut entries = self
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry");
        let backend = entries
            .get_mut(&process_id)
            .filter(|backend| backend.secret_key == secret_key)
            .ok_or_else(|| super::CassieError::Execution("unknown pgwire backend".into()))?;
        if backend.aggregate_pause.is_some() {
            return Err(super::CassieError::Execution(
                "aggregate pause already pending".into(),
            ));
        }
        let (pause, selected, resume) = super::aggregate_test_pause::QueryAggregateTestPause::new();
        let pause = Arc::new(pause);
        backend.aggregate_pause = Some(Arc::clone(&pause));
        Ok((
            super::QueryAggregateTestPauseGuard {
                runtime: Arc::downgrade(self),
                process_id,
                pause,
            },
            selected,
            resume,
        ))
    }

    pub(super) fn clear_backend_aggregate_pause(
        &self,
        process_id: i32,
        pause: &Arc<super::aggregate_test_pause::QueryAggregateTestPause>,
    ) {
        if let Some(backend) = self
            .pgwire_backends
            .entries
            .lock()
            .expect("pgwire backend registry")
            .get_mut(&process_id)
        {
            let is_same = backend
                .aggregate_pause
                .as_ref()
                .is_some_and(|pending| Arc::ptr_eq(pending, pause));
            if is_same {
                backend.aggregate_pause = None;
            }
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
    use crate::runtime::QueryExecutionControls;
    use std::time::{Duration, Instant};

    #[test]
    fn should_attach_aggregate_pause_only_to_the_selected_instance_and_backend() {
        // Arrange
        let first = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let second = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = first.register_pgwire_backend();
        let sibling = first.register_pgwire_backend();
        let other = second.register_pgwire_backend();
        let (guard, selected, resume) = first
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm target backend");

        // Act
        let bare = QueryCancellationHandle::new();
        let sibling_query = sibling.begin_query();
        let other_query = other.begin_query();
        let target_query = target.begin_query();
        drop(guard);
        let limits = CassieRuntimeLimits::default();
        for cancellation in [bare, sibling_query.handle(), other_query.handle()] {
            QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation)
                .aggregate_input_retained()
                .expect("unrelated query does not pause");
        }
        let target_controls = QueryExecutionControls::with_cancellation(
            &limits,
            Instant::now(),
            target_query.handle(),
        );
        let worker = std::thread::spawn(move || target_controls.aggregate_input_retained());
        let reached = selected.recv_timeout(Duration::from_secs(1));
        resume.send(()).expect("resume target");

        // Assert
        reached.expect("selected backend reaches retained input");
        assert!(worker.join().expect("target worker").is_ok());
    }

    #[test]
    fn should_clear_only_the_unclaimed_backend_pause_when_its_guard_drops() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = runtime.register_pgwire_backend();
        let (guard, selected, _resume) = runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm target backend");

        // Act
        drop(guard);
        let query = target.begin_query();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            query.handle(),
        );

        // Assert
        assert!(controls.aggregate_input_retained().is_ok());
        assert!(matches!(
            selected.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn should_reject_wrong_backend_identity_and_duplicate_pending_pause() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = runtime.register_pgwire_backend();
        let wrong = target.secret_key().wrapping_add(1);

        // Act
        let invalid = runtime.pause_backend_after_aggregate_input(target.process_id(), wrong);
        let (guard, _selected, _resume) = runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm backend");
        let duplicate =
            runtime.pause_backend_after_aggregate_input(target.process_id(), target.secret_key());

        // Assert
        assert!(invalid.is_err());
        assert!(duplicate.is_err());
        drop(guard);
        assert!(runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .is_ok());
    }

    #[test]
    fn should_release_unclaimed_pause_when_backend_unregisters() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = runtime.register_pgwire_backend();
        let process_id = target.process_id();
        let secret_key = target.secret_key();
        let (guard, selected, _resume) = runtime
            .pause_backend_after_aggregate_input(process_id, secret_key)
            .expect("arm backend");

        // Act
        drop(target);
        drop(guard);

        // Assert
        assert!(matches!(
            selected.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        assert!(runtime
            .pause_backend_after_aggregate_input(process_id, secret_key)
            .is_err());
    }

    #[test]
    fn should_keep_aggregate_and_sleep_pause_on_the_same_backend_query() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = runtime.register_pgwire_backend();
        let (_aggregate_guard, aggregate_reached, aggregate_resume) = runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm aggregate pause");
        let (_sleep_guard, sleep_reached, sleep_resume) =
            crate::runtime::pause_next_query_at_sleep_entry();
        let query = target.begin_query();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            query.handle(),
        );
        let worker = std::thread::spawn(move || {
            controls.sleep_function_entered()?;
            controls.aggregate_input_retained()
        });

        // Act
        let sleep = sleep_reached.recv_timeout(Duration::from_secs(1));
        sleep_resume.send(()).expect("resume sleep boundary");
        let aggregate = aggregate_reached.recv_timeout(Duration::from_secs(1));
        aggregate_resume
            .send(())
            .expect("resume aggregate boundary");

        // Assert
        sleep.expect("sleep boundary reached");
        aggregate.expect("aggregate boundary reached on the same query");
        assert!(worker.join().expect("query worker").is_ok());
    }

    #[test]
    fn should_keep_claimed_pause_in_clones_when_old_guard_drops() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = runtime.register_pgwire_backend();
        let (old_guard, old_reached, old_resume) = runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm first pause");
        let old_query = target.begin_query();
        let old_handle = old_query.handle();
        let clone = old_handle.clone();
        let (new_guard, new_reached, new_resume) = runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm replacement pause");

        // Act
        drop(old_guard);
        let new_query = target.begin_query();
        drop(new_guard);
        let limits = CassieRuntimeLimits::default();
        let old_controls =
            QueryExecutionControls::with_cancellation(&limits, Instant::now(), clone);
        let old_worker = std::thread::spawn(move || {
            old_controls.aggregate_input_retained()?;
            old_controls.aggregate_input_retained()
        });
        let old_result = old_reached.recv_timeout(Duration::from_secs(1));
        old_resume.send(()).expect("resume first pause");
        let new_controls =
            QueryExecutionControls::with_cancellation(&limits, Instant::now(), new_query.handle());
        let new_worker = std::thread::spawn(move || new_controls.aggregate_input_retained());
        let new_result = new_reached.recv_timeout(Duration::from_secs(1));
        new_resume.send(()).expect("resume replacement pause");

        // Assert
        old_result.expect("claimed clone reaches the first pause");
        new_result.expect("dropping the old guard preserves the replacement");
        assert!(old_worker.join().expect("first worker").is_ok());
        assert!(new_worker.join().expect("replacement worker").is_ok());
    }

    #[test]
    fn should_leave_pending_pause_for_a_new_query_when_a_portal_resumes() {
        // Arrange
        let runtime = Arc::new(RuntimeState::new(CassieRuntimeLimits::default()));
        let target = runtime.register_pgwire_backend();
        let suspended = target.begin_query().suspend();
        let (_guard, selected, resume) = runtime
            .pause_backend_after_aggregate_input(target.process_id(), target.secret_key())
            .expect("arm backend");

        // Act
        let resumed = target.resume_query(suspended);
        let limits = CassieRuntimeLimits::default();
        let resumed_controls =
            QueryExecutionControls::with_cancellation(&limits, Instant::now(), resumed.handle());
        let resumed_result = resumed_controls.aggregate_input_retained();
        let query = target.begin_query();
        let controls =
            QueryExecutionControls::with_cancellation(&limits, Instant::now(), query.handle());
        let worker = std::thread::spawn(move || controls.aggregate_input_retained());
        let reached = selected.recv_timeout(Duration::from_secs(1));
        resume.send(()).expect("resume new query");

        // Assert
        assert!(resumed_result.is_ok());
        reached.expect("new query claims the pending pause");
        assert!(worker.join().expect("worker").is_ok());
    }

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
