use super::{
    Arc, AtomicBool, AtomicUsize, CassieError, CassieRuntimeLimits, Duration, Instant, Ordering,
    RuntimeState,
};
use std::sync::Weak;

#[derive(Debug, Clone)]
pub struct QueryCancellationHandle(
    Arc<AtomicBool>,
    Option<Arc<crate::midge::adapter::QueryScanControlScope>>,
    Option<Arc<super::scoring_test_pause::ScoringTestPause>>,
);

impl Default for QueryCancellationHandle {
    fn default() -> Self {
        Self(
            Arc::new(AtomicBool::new(false)),
            crate::midge::adapter::current_query_scan_control_scope(),
            None,
        )
    }
}

impl QueryCancellationHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pauses this request once after a candidate is scored and retained.
    /// Install before cloning the handle. Unrelated requests have no pause.
    /// Both endpoints must be serviced; a disconnected or stalled observer fails
    /// the diagnostic query rather than hanging the process indefinitely.
    #[doc(hidden)]
    pub fn pause_after_scored_candidate(
        &mut self,
        boundary: super::QueryScoringTestBoundary,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ) {
        let (pause, selected, resume) = super::scoring_test_pause::ScoringTestPause::new(boundary);
        self.2 = Some(Arc::new(pause));
        (selected, resume)
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
        *self
            .query_memory_observation
            .lock()
            .expect("query memory observation") =
            QueryMemoryObservation(Arc::downgrade(&controls.memory));
        metrics.query.peak_accounted_memory_bytes = metrics
            .query
            .peak_accounted_memory_bytes
            .max(controls.peak_query_memory_bytes() as u64);
    }
}

/// Observes only the latest recorded query without retaining its tracker or request scope.
#[derive(Debug, Default)]
pub(super) struct QueryMemoryObservation(Weak<QueryMemoryTracker>);

impl QueryMemoryObservation {
    pub(super) fn current_and_peak_bytes(&self) -> (usize, usize) {
        self.0.upgrade().map_or((0, 0), |tracker| {
            let current = tracker.used.load(Ordering::Acquire);
            let peak = tracker.peak.load(Ordering::Acquire).max(current);
            (current, peak)
        })
    }
}

#[derive(Debug, Clone)]
pub struct QueryExecutionControls {
    pub deadline: Option<Instant>,
    pub max_result_rows: usize,
    pub query_memory_budget_bytes: usize,
    pub cte_recursion_depth: usize,
    cancellation: QueryCancellationHandle,
    relational_cte_boundary: bool,
    memory: Arc<QueryMemoryTracker>,
    statement_read: Option<Arc<crate::midge::adapter::StatementDataRead>>,
    /// The statement's `TimeZone`, `DateStyle` and clocks, shared by clones.
    temporal:
        Arc<parking_lot::RwLock<Option<Arc<crate::types::extended::temporal::TemporalSession>>>>,
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
    /// Creates a separate narrow lease against the same query tracker.
    pub(crate) fn try_sibling(&self, bytes: usize) -> Result<Self, CassieError> {
        reserve_tracker_memory(&self.tracker, bytes)?;
        Ok(Self {
            tracker: Arc::clone(&self.tracker),
            bytes,
        })
    }

    pub(crate) fn shares_tracker(&self, controls: &QueryExecutionControls) -> bool {
        Arc::ptr_eq(&self.tracker, &controls.memory)
    }

    /// Transfers an already-live reservation without releasing or reserving its bytes again.
    pub(crate) fn absorb(&mut self, mut other: Self) {
        assert!(Arc::ptr_eq(&self.tracker, &other.tracker));
        self.bytes = self
            .bytes
            .checked_add(other.bytes)
            .expect("admitted memory sum");
        other.bytes = 0;
    }
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
    pub(crate) fn scored_candidate(
        &self,
        boundary: super::QueryScoringTestBoundary,
    ) -> Result<(), CassieError> {
        if let Some(pause) = &self.cancellation.2 {
            pause.reach(boundary)?;
        }
        Ok(())
    }

    pub(crate) fn check_interruption(&self) -> Result<(), CassieError> {
        if self.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if self.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        Ok(())
    }
    pub(crate) fn with_statement_read(
        &self,
        owner: Option<Arc<crate::midge::adapter::StatementDataRead>>,
    ) -> Self {
        Self {
            statement_read: owner,
            ..self.clone()
        }
    }

    pub(crate) fn statement_read(&self) -> Option<&Arc<crate::midge::adapter::StatementDataRead>> {
        self.statement_read.as_ref()
    }

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
            statement_read: None,
            deadline,
            max_result_rows: limits.max_result_rows,
            query_memory_budget_bytes: limits.query_memory_budget_bytes,
            cte_recursion_depth: limits.cte_recursion_depth,
            cancellation,
            relational_cte_boundary: false,
            temporal: Arc::default(),
            memory: Arc::new(QueryMemoryTracker {
                budget: limits.query_memory_budget_bytes,
                used: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }),
        }
    }

    /// Installs the statement's temporal session for evaluation and output.
    pub(crate) fn install_temporal_session(
        &self,
        session: Arc<crate::types::extended::temporal::TemporalSession>,
    ) {
        *self.temporal.write() = Some(session);
    }

    /// The statement's temporal session, when one was installed.
    #[must_use]
    pub(crate) fn temporal_session(
        &self,
    ) -> Option<Arc<crate::types::extended::temporal::TemporalSession>> {
        self.temporal.read().clone()
    }

    /// Returns controls for an input relation that must be fully
    /// materialized, such as a set-operation branch: the result-row cap
    /// applies only to the final result, while the deadline, cancellation and
    /// shared memory budget still apply.
    #[must_use]
    pub fn for_uncapped_input(&self) -> Self {
        Self {
            max_result_rows: usize::MAX,
            ..self.clone()
        }
    }

    pub(crate) fn query_scan_control_scope(
        &self,
    ) -> Option<&crate::midge::adapter::QueryScanControlScope> {
        self.cancellation.1.as_deref()
    }

    pub(crate) fn for_relational_scalar_cte(&self) -> Self {
        Self {
            relational_cte_boundary: true,
            ..self.clone()
        }
    }

    pub(crate) const fn uses_relational_cte_boundary(&self) -> bool {
        self.relational_cte_boundary
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

    #[test]
    fn should_observe_query_memory_until_the_last_result_alias_is_dropped() {
        // Arrange
        let runtime = super::RuntimeState::new(CassieRuntimeLimits::default());
        let controls = controls_with_budget(1_048_576);
        let tracker = std::sync::Arc::downgrade(&controls.memory);
        let mut builder = crate::executor::QueryResult::try_builder(&controls).expect("builder");
        builder
            .try_push_row(&[crate::types::Value::String("retained output".repeat(32))])
            .expect("row");
        let result = builder.finish().expect("result");
        let alias = result.clone();
        runtime.record_query_memory(&controls);

        // Act
        let retained = runtime.snapshot().query.current_accounted_memory_bytes;
        drop(controls);
        drop(result);
        let aliased = runtime.snapshot().query.current_accounted_memory_bytes;
        drop(alias);
        let retired = runtime.snapshot().query.current_accounted_memory_bytes;
        let tracker_retired = tracker.upgrade().is_none();

        // Assert
        assert!(retained > 0);
        assert_eq!(aliased, retained);
        assert_eq!(retired, 0);
        assert!(tracker_retired);
    }

    #[test]
    fn should_observe_memory_peak_when_a_retained_output_lease_grows() {
        // Arrange
        let runtime = super::RuntimeState::new(CassieRuntimeLimits::default());
        let controls = controls_with_budget(1_048_576);
        let mut builder = crate::executor::QueryResult::try_builder(&controls).expect("builder");
        builder
            .try_push_row(&[crate::types::Value::String("output".to_owned())])
            .expect("row");
        let result = builder.finish().expect("result");
        runtime.record_query_memory(&controls);
        let recorded_peak = runtime.snapshot().query.peak_accounted_memory_bytes;

        // Act
        // Frontend frame copies reserve sibling leases after query execution is recorded.
        let scratch = result
            .admission()
            .try_sibling(8192)
            .expect("frontend scratch");
        let live = runtime.snapshot().query;
        let tracker_peak = controls.peak_query_memory_bytes() as u64;
        drop(scratch);
        drop(result);
        drop(controls);
        let retired = runtime.snapshot().query;

        // Assert
        assert!(live.current_accounted_memory_bytes > recorded_peak);
        assert!(live.peak_accounted_memory_bytes >= live.current_accounted_memory_bytes);
        assert_eq!(live.peak_accounted_memory_bytes, tracker_peak);
        assert_eq!(retired.current_accounted_memory_bytes, 0);
        assert_eq!(retired.peak_accounted_memory_bytes, tracker_peak);
    }

    #[test]
    fn should_retire_cancellation_scope_while_managed_rows_keep_only_memory_alive() {
        // Arrange
        let cancellation = super::QueryCancellationHandle::new();
        let cancellation_owner = std::sync::Arc::downgrade(&cancellation.0);
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let tracker = std::sync::Arc::downgrade(&controls.memory);
        let mut builder = crate::executor::QueryResult::try_builder(&controls).expect("builder");
        builder
            .try_push_column(&crate::executor::ColumnMeta::text("owned"))
            .expect("metadata");
        builder
            .try_push_row(&[
                crate::types::Value::String("variable payload".repeat(32)),
                crate::types::Value::Json(serde_json::json!({"nested": ["retained", 7]})),
                crate::types::Value::Vector(crate::types::Vector::new(vec![0.5; 128])),
            ])
            .expect("variable-width values");
        let rows = builder.finish().expect("result").into_owned_rows();
        let alias = rows.clone();

        // Act
        drop(controls);
        drop(cancellation);
        drop(rows);
        let request_retired = cancellation_owner.upgrade().is_none();
        let memory_live = tracker
            .upgrade()
            .expect("narrow retained tracker")
            .used
            .load(super::Ordering::Acquire);
        let retained_values = alias.as_slice()[0].len();
        drop(alias);
        let tracker_retired = tracker.upgrade().is_none();

        // Assert
        assert!(request_retired);
        assert!(memory_live > 0);
        assert_eq!(retained_values, 3);
        assert!(tracker_retired);
    }
}
