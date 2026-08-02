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

    pub(crate) fn record_query_memory(&self, controls: &QueryExecutionControls) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.current_accounted_memory_bytes = controls.current_query_memory_bytes() as u64;
        metrics.query.peak_accounted_memory_bytes = metrics
            .query
            .peak_accounted_memory_bytes
            .max(controls.peak_query_memory_bytes() as u64);
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

impl QueryMemoryReservation {
    #[must_use]
    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    /// # Errors
    ///
    /// Returns a resource-limit error when the shared query budget cannot satisfy the growth.
    ///
    /// # Panics
    ///
    /// Panics if the shared tracker accepts growth that overflows this reservation. This indicates
    /// an internal accounting invariant violation.
    pub(crate) fn try_grow(&mut self, additional_bytes: usize) -> Result<(), CassieError> {
        reserve_tracker_memory(&self.tracker, additional_bytes)?;
        self.bytes = self
            .bytes
            .checked_add(additional_bytes)
            .expect("reservation growth is bounded by the shared tracker");
        Ok(())
    }

    /// # Panics
    ///
    /// Panics when `retained_bytes` exceeds the current reservation.
    pub(crate) fn shrink_to(&mut self, retained_bytes: usize) {
        assert!(
            retained_bytes <= self.bytes,
            "cannot grow a query memory reservation by shrinking it"
        );
        let released = self.bytes - retained_bytes;
        self.bytes = retained_bytes;
        self.tracker.used.fetch_sub(released, Ordering::AcqRel);
    }
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
        reserve_tracker_memory(&self.memory, bytes)?;
        Ok(QueryMemoryReservation {
            tracker: Arc::clone(&self.memory),
            bytes,
        })
    }

    #[must_use]
    pub fn peak_query_memory_bytes(&self) -> usize {
        self.memory.peak.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn current_query_memory_bytes(&self) -> usize {
        self.memory.used.load(Ordering::Acquire)
    }
}

fn reserve_tracker_memory(tracker: &QueryMemoryTracker, bytes: usize) -> Result<(), CassieError> {
    let mut used = tracker.used.load(Ordering::Acquire);
    loop {
        let Some(next) = used.checked_add(bytes) else {
            return Err(memory_limit_error(tracker.budget, usize::MAX));
        };
        if next > tracker.budget {
            return Err(memory_limit_error(tracker.budget, next));
        }
        match tracker
            .used
            .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                tracker.peak.fetch_max(next, Ordering::AcqRel);
                return Ok(());
            }
            Err(actual) => used = actual,
        }
    }
}

fn memory_limit_error(budget: usize, requested: usize) -> CassieError {
    CassieError::ResourceLimit(format!(
        "query memory budget exceeded: {requested} > {budget}"
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::QueryExecutionControls;
    use crate::config::CassieRuntimeLimits;

    fn controls_with_budget(bytes: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: bytes,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    #[test]
    fn should_fail_exactly_at_the_query_memory_budget() {
        // Arrange
        let controls = controls_with_budget(8);

        // Act
        let reservation = controls.reserve_query_memory(8);
        let overflow = controls.reserve_query_memory(1);

        // Assert
        assert_eq!(reservation.expect("exact budget").bytes(), 8);
        assert!(overflow.is_err());
    }

    #[test]
    fn should_release_shrunk_query_memory_reservations() {
        // Arrange
        let controls = controls_with_budget(8);
        let mut reservation = controls.reserve_query_memory(8).expect("full budget");

        // Act
        reservation.shrink_to(3);
        let replacement = controls.reserve_query_memory(5);

        // Assert
        assert_eq!(reservation.bytes(), 3);
        assert_eq!(replacement.expect("released budget").bytes(), 5);
    }

    #[test]
    fn should_reject_reservation_growth_given_integer_overflow() {
        // Arrange
        let controls = controls_with_budget(usize::MAX);
        let mut reservation = controls.reserve_query_memory(1).expect("first byte");

        // Act
        let overflow = reservation.try_grow(usize::MAX);

        // Assert
        assert!(overflow.is_err());
        assert_eq!(reservation.bytes(), 1);
    }
}
