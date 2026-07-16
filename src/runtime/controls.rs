use super::{
    Arc, AtomicBool, AtomicUsize, CassieError, CassieRuntimeLimits, Duration, Instant, Ordering,
    RuntimeState,
};

#[derive(Debug, Clone, Default)]
pub struct QueryCancellationHandle(Arc<AtomicBool>);

impl QueryCancellationHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) fn is_same_request(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl RuntimeState {
    pub(crate) fn record_query_peak_memory(&self, peak_bytes: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.peak_accounted_memory_bytes = metrics
            .query
            .peak_accounted_memory_bytes
            .max(peak_bytes as u64);
    }
}

#[derive(Debug, Clone)]
pub struct QueryExecutionControls {
    pub deadline: Option<Instant>,
    pub max_result_rows: usize,
    pub query_memory_budget_bytes: usize,
    pub cte_recursion_depth: usize,
    cancellation: QueryCancellationHandle,
    memory: Arc<QueryMemoryTracker>,
}

#[derive(Debug)]
struct QueryMemoryTracker {
    budget: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
}

#[derive(Debug)]
pub struct QueryMemoryReservation {
    tracker: Arc<QueryMemoryTracker>,
    bytes: usize,
}

impl Drop for QueryMemoryReservation {
    fn drop(&mut self) {
        self.tracker.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

impl QueryExecutionControls {
    #[must_use]
    pub fn from_limits(limits: &CassieRuntimeLimits, started_at: Instant) -> Self {
        Self::with_cancellation(limits, started_at, QueryCancellationHandle::new())
    }

    #[must_use]
    pub fn with_cancellation(
        limits: &CassieRuntimeLimits,
        started_at: Instant,
        cancellation: QueryCancellationHandle,
    ) -> Self {
        let deadline = if limits.query_timeout_ms == 0 {
            None
        } else {
            started_at
                .checked_add(Duration::from_millis(limits.query_timeout_ms))
                .or(Some(started_at))
        };

        Self {
            deadline,
            max_result_rows: limits.max_result_rows,
            query_memory_budget_bytes: limits.query_memory_budget_bytes,
            cte_recursion_depth: limits.cte_recursion_depth,
            cancellation,
            memory: Arc::new(QueryMemoryTracker {
                budget: limits.query_memory_budget_bytes,
                used: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }),
        }
    }

    #[must_use]
    pub fn is_timed_out(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// # Errors
    ///
    /// Returns a resource-limit error when the shared query budget cannot satisfy the request.
    pub fn reserve_query_memory(
        &self,
        bytes: usize,
    ) -> Result<QueryMemoryReservation, CassieError> {
        let mut used = self.memory.used.load(Ordering::Acquire);
        loop {
            let Some(next) = used.checked_add(bytes) else {
                return Err(self.memory_limit_error(usize::MAX));
            };
            if next > self.memory.budget {
                return Err(self.memory_limit_error(next));
            }
            match self.memory.used.compare_exchange_weak(
                used,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.memory.peak.fetch_max(next, Ordering::AcqRel);
                    return Ok(QueryMemoryReservation {
                        tracker: Arc::clone(&self.memory),
                        bytes,
                    });
                }
                Err(actual) => used = actual,
            }
        }
    }

    #[must_use]
    pub fn peak_query_memory_bytes(&self) -> usize {
        self.memory.peak.load(Ordering::Acquire)
    }

    fn memory_limit_error(&self, requested: usize) -> CassieError {
        CassieError::ResourceLimit(format!(
            "query memory budget exceeded: {requested} > {}",
            self.memory.budget
        ))
    }
}
