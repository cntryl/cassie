use super::{Arc, CassieError, CassieRuntimeLimits, Duration, Instant, RuntimeState};
use cassie_core::request::{
    Interruption, InterruptionError, RequestCancellation, RequestMemory, RequestMemoryReservation,
    RequestMemoryTracker,
};
use std::sync::Weak;

#[derive(Debug, Clone)]
pub struct QueryCancellationHandle(
    RequestCancellation,
    Option<Arc<crate::midge::adapter::QueryScanControlScope>>,
    Option<Arc<super::QueryTestPause>>,
    Option<Arc<super::aggregate_test_pause::QueryAggregateTestPause>>,
);

impl Default for QueryCancellationHandle {
    fn default() -> Self {
        Self(
            RequestCancellation::new(),
            crate::midge::adapter::current_query_scan_control_scope(),
            super::sleep_test_pause::take_next_query_pause()
                .or_else(super::regex_test_pause::take_next_query_pause),
            None,
        )
    }
}

impl QueryCancellationHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) const fn request_cancellation(&self) -> &RequestCancellation {
        &self.0
    }

    pub(super) fn attach_aggregate_test_pause(
        &mut self,
        pause: Arc<super::aggregate_test_pause::QueryAggregateTestPause>,
    ) -> bool {
        if self.3.is_some() {
            return false;
        }
        self.3 = Some(pause);
        true
    }

    #[doc(hidden)]
    pub fn pause_after_copy_row_staged(&mut self) -> super::CopyTestPauseGuard {
        assert!(self.2.is_none(), "COPY test pauses must not be nested");
        let (pause, staged, resume) = super::copy_test_pause::CopyTestPause::new();
        let pause = Arc::new(pause);
        self.2 = Some(Arc::new(super::QueryTestPause::Copy(Arc::clone(&pause))));
        super::CopyTestPauseGuard::new(Arc::downgrade(&pause), staged, resume)
    }

    #[cfg(test)]
    pub(super) fn attach_copy_test_pause(
        &mut self,
        pause: Arc<super::copy_test_pause::CopyTestPause>,
    ) -> bool {
        if self.2.is_some() {
            return false;
        }
        self.2 = Some(Arc::new(super::QueryTestPause::Copy(pause)));
        true
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
        self.2 = Some(Arc::new(super::QueryTestPause::Scoring(pause)));
        (selected, resume)
    }

    pub fn cancel(&self) {
        self.0.cancel();
    }

    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.0.is_canceled()
    }

    #[doc(hidden)]
    pub async fn wait_canceled(&self) {
        self.0.wait_canceled().await;
    }

    pub(crate) fn is_same_request(&self, other: &Self) -> bool {
        self.0.is_same_request(&other.0)
    }
}

impl RuntimeState {
    #[doc(hidden)]
    pub fn record_query_peak_memory(&self, peak_bytes: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.peak_accounted_memory_bytes = metrics
            .query
            .peak_accounted_memory_bytes
            .max(peak_bytes as u64);
    }

    #[doc(hidden)]
    pub fn record_query_memory(&self, controls: &QueryExecutionControls) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.current_accounted_memory_bytes = controls.current_query_memory_bytes() as u64;
        *self
            .query_memory_observation
            .lock()
            .expect("query memory observation") =
            QueryMemoryObservation(controls.memory.downgrade());
        metrics.query.peak_accounted_memory_bytes = metrics
            .query
            .peak_accounted_memory_bytes
            .max(controls.peak_query_memory_bytes() as u64);
    }
}

/// Observes only the latest recorded query without retaining its tracker or request scope.
#[derive(Debug, Default)]
pub(super) struct QueryMemoryObservation(Weak<RequestMemoryTracker>);

impl QueryMemoryObservation {
    pub(super) fn current_and_peak_bytes(&self) -> (usize, usize) {
        self.0.upgrade().map_or((0, 0), |tracker| {
            let current = tracker.current_bytes();
            let peak = tracker.peak_bytes().max(current);
            (current, peak)
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OrderedJoinCounts {
    pub order_index_entries: usize,
    pub order_rows: usize,
    pub dimension_probes: usize,
    pub dimension_base_fetches: usize,
    pub intermediate_rows: usize,
    pub fully_joined_rows: usize,
    pub joined_rows: usize,
}

#[derive(Debug, Default)]
struct OrderedReadObservation {
    join: Option<OrderedJoinCounts>,
    proof: Option<(usize, &'static str)>,
}

type ExecutorCache = parking_lot::Mutex<Option<Box<dyn std::any::Any + Send>>>;

#[derive(Debug, Default)]
struct StatementControlState {
    // A plain SQL request needs neither the graph cache nor an EXPLAIN observer.
    executor: std::sync::OnceLock<Arc<ExecutorCache>>,
    ordered_join: Option<Arc<parking_lot::Mutex<OrderedReadObservation>>>,
}

impl StatementControlState {
    fn executor(&self) -> &Arc<ExecutorCache> {
        self.executor.get_or_init(Arc::default)
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
    memory: RequestMemory,
    statement_read: Option<Arc<crate::midge::adapter::StatementDataRead>>,
    /// The statement's `TimeZone`, `DateStyle` and clocks, shared by clones.
    temporal: Arc<parking_lot::RwLock<TemporalSlot>>,
    /// Statement-scoped state of one executor subsystem, shared by clones and released with
    /// the last clone, so its reservations end with the statement.
    statement_state: Arc<StatementControlState>,
}

#[derive(Debug, Default)]
struct TemporalSlot {
    session: Option<Arc<crate::types::extended::temporal::TemporalSession>>,
    is_posix: bool,
}

#[derive(Debug)]
pub struct QueryMemoryReservation {
    core: RequestMemoryReservation,
}

impl QueryMemoryReservation {
    pub(crate) fn from_core(core: RequestMemoryReservation) -> Self {
        Self { core }
    }

    pub(crate) fn into_core(self) -> RequestMemoryReservation {
        self.core
    }

    pub(crate) const fn as_core(&self) -> &RequestMemoryReservation {
        &self.core
    }

    pub(crate) const fn as_core_mut(&mut self) -> &mut RequestMemoryReservation {
        &mut self.core
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    pub(crate) fn bytes_for_test(&self) -> usize {
        self.core.bytes()
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn belongs_to(&self, controls: &QueryExecutionControls) -> bool {
        self.core.belongs_to(&controls.memory)
    }

    #[cfg(not(any(test, feature = "transport-test-hooks")))]
    pub(crate) fn belongs_to(&self, controls: &QueryExecutionControls) -> bool {
        self.core.belongs_to(&controls.memory)
    }

    /// Creates a separate narrow lease against the same query tracker.
    /// # Errors
    ///
    /// Returns a resource-limit error when the shared query budget cannot satisfy the request.
    #[doc(hidden)]
    pub fn try_sibling(&self, bytes: usize) -> Result<Self, CassieError> {
        self.core
            .try_sibling(bytes)
            .map(|core| Self { core })
            .map_err(memory_budget_error)
    }

    pub(crate) fn shares_tracker(&self, controls: &QueryExecutionControls) -> bool {
        self.core.belongs_to(&controls.memory)
    }

    /// Transfers an already-live reservation without releasing or reserving its bytes again.
    /// # Panics
    ///
    /// Panics when the reservations belong to different query trackers.
    #[doc(hidden)]
    pub fn absorb(&mut self, other: Self) {
        self.core.absorb(other.core);
    }
    #[must_use]
    #[doc(hidden)]
    pub const fn bytes(&self) -> usize {
        self.core.bytes()
    }

    /// # Errors
    ///
    /// Returns a resource-limit error when the shared query budget cannot satisfy the growth.
    ///
    /// # Panics
    ///
    /// Panics if the shared tracker accepts growth that overflows this reservation. This indicates
    /// an internal accounting invariant violation.
    #[doc(hidden)]
    pub fn try_grow(&mut self, additional_bytes: usize) -> Result<(), CassieError> {
        self.core
            .try_grow(additional_bytes)
            .map_err(memory_budget_error)
    }

    /// # Panics
    ///
    /// Panics when `retained_bytes` exceeds the current reservation.
    #[doc(hidden)]
    pub fn shrink_to(&mut self, retained_bytes: usize) {
        assert!(
            retained_bytes <= self.core.bytes(),
            "cannot grow a query memory reservation by shrinking it"
        );
        self.core.shrink_to(retained_bytes);
    }
}

impl QueryExecutionControls {
    pub(crate) fn observe_ordered_join(&self) -> Self {
        Self {
            statement_state: Arc::new(StatementControlState {
                executor: Arc::clone(self.statement_state.executor()).into(),
                ordered_join: Some(Arc::default()),
            }),
            ..self.clone()
        }
    }

    pub(crate) fn record_order_proof(&self, visited: usize, reason: &'static str) {
        if let Some(slot) = &self.statement_state.ordered_join {
            slot.lock().proof = Some((visited, reason));
        }
    }

    pub(crate) fn order_proof(&self) -> Option<(usize, &'static str)> {
        self.statement_state
            .ordered_join
            .as_ref()
            .and_then(|slot| slot.lock().proof)
    }

    pub(crate) fn record_ordered_join(&self, counts: OrderedJoinCounts) {
        if let Some(slot) = &self.statement_state.ordered_join {
            slot.lock().join = Some(counts);
        }
    }

    pub(crate) fn ordered_join_counts(&self) -> Option<OrderedJoinCounts> {
        self.statement_state
            .ordered_join
            .as_ref()
            .and_then(|slot| slot.lock().join)
    }

    pub(crate) fn aggregate_input_retained(&self) -> Result<(), CassieError> {
        if let Some(pause) = self.cancellation.3.as_deref() {
            pause.reach()?;
        }
        Ok(())
    }

    pub(crate) fn sleep_function_entered(&self) -> Result<(), CassieError> {
        if let Some(super::QueryTestPause::Sleep(pause)) = self.cancellation.2.as_deref() {
            pause.reach()?;
        }
        Ok(())
    }

    pub(crate) fn regex_search_completed(&self, matched: bool) -> Result<(), CassieError> {
        if let Some(super::QueryTestPause::Regex(pause)) = self.cancellation.2.as_deref() {
            pause.reach(matched)?;
        }
        Ok(())
    }

    pub(crate) fn scored_candidate(
        &self,
        boundary: super::QueryScoringTestBoundary,
    ) -> Result<(), CassieError> {
        if let Some(super::QueryTestPause::Scoring(pause)) = self.cancellation.2.as_deref() {
            pause.reach(boundary)?;
        }
        Ok(())
    }

    pub(crate) fn copy_row_staged(&self, ordinal: usize) -> Result<(), CassieError> {
        if let Some(super::QueryTestPause::Copy(pause)) = self.cancellation.2.as_deref() {
            pause.reach(ordinal)?;
        }
        Ok(())
    }

    /// Checks whether this query's cancellation or deadline has fired.
    ///
    /// # Errors
    ///
    /// Returns the cancellation or deadline error when execution should stop.
    #[doc(hidden)]
    pub fn check_interruption(&self) -> Result<(), CassieError> {
        self.interruption().check().map_err(|error| match error {
            InterruptionError::Canceled => CassieError::QueryCanceled,
            InterruptionError::DeadlineExceeded => CassieError::DeadlineExceeded,
        })
    }

    #[must_use]
    pub fn interruption(&self) -> Interruption<'_> {
        Interruption::new(self.deadline, &self.cancellation.0)
    }

    #[must_use]
    pub fn request_memory(&self) -> &RequestMemory {
        &self.memory
    }

    /// Reserves request-owned memory from the configured query budget.
    ///
    /// # Errors
    /// Returns `CassieError::ResourceLimit` if the requested bytes exceed the
    /// request budget or the reservation cannot be represented.
    pub fn reserve_request_memory(
        &self,
        bytes: usize,
    ) -> Result<RequestMemoryReservation, CassieError> {
        self.memory
            .reserve(bytes)
            .map_err(memory_budget_error)
            .map(|reservation| QueryMemoryReservation::from_core(reservation).into_core())
    }
    pub(crate) fn with_statement_read(
        &self,
        owner: Option<Arc<crate::midge::adapter::StatementDataRead>>,
    ) -> Self {
        Self {
            statement_read: owner,
            // Each captured read owns its cache. EXPLAIN observes the whole statement.
            statement_state: Arc::new(StatementControlState {
                executor: std::sync::OnceLock::new(),
                ordered_join: self.statement_state.ordered_join.clone(),
            }),
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
            statement_state: Arc::default(),
            memory: RequestMemory::new(limits.query_memory_budget_bytes),
        }
    }

    /// Runs `visit` with the statement-scoped state slot locked.
    pub(crate) fn with_statement_state<R>(
        &self,
        visit: impl FnOnce(&mut Option<Box<dyn std::any::Any + Send>>) -> R,
    ) -> R {
        visit(&mut self.statement_state.executor().lock())
    }

    /// Installs the statement's temporal session for evaluation and output.
    pub(crate) fn install_temporal_session(
        &self,
        session: Arc<crate::types::extended::temporal::TemporalSession>,
        is_posix: bool,
    ) {
        let mut slot = self.temporal.write();
        slot.session = Some(session);
        slot.is_posix = is_posix;
    }

    /// The statement's temporal session, when one was installed.
    #[must_use]
    pub(crate) fn temporal_session(
        &self,
    ) -> Option<Arc<crate::types::extended::temporal::TemporalSession>> {
        self.temporal.read().session.clone()
    }

    pub(crate) fn temporal_session_is_posix(
        &self,
        session: &crate::types::extended::temporal::TemporalSession,
    ) -> Result<bool, CassieError> {
        loop {
            self.check_interruption()?;
            if let Some(slot) = self.temporal.try_read_for(Duration::from_millis(2)) {
                self.check_interruption()?;
                return Ok(slot.is_posix
                    && slot
                        .session
                        .as_deref()
                        .is_some_and(|current| std::ptr::eq(current, session)));
            }
        }
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
    pub fn is_canceled(&self) -> bool {
        self.cancellation.is_canceled()
    }

    /// # Errors
    ///
    /// Returns a resource-limit error when the shared query budget cannot satisfy the request.
    pub fn reserve_query_memory(
        &self,
        bytes: usize,
    ) -> Result<QueryMemoryReservation, CassieError> {
        self.reserve_request_memory(bytes)
            .map(QueryMemoryReservation::from_core)
    }

    #[must_use]
    pub fn peak_query_memory_bytes(&self) -> usize {
        self.memory.peak_bytes()
    }

    #[must_use]
    pub fn current_query_memory_bytes(&self) -> usize {
        self.memory.current_bytes()
    }
}

fn memory_limit_error(budget: usize, requested: usize) -> CassieError {
    CassieError::ResourceLimit(format!(
        "query memory budget exceeded: {requested} > {budget}"
    ))
}

fn memory_budget_error(error: cassie_core::request::MemoryBudgetError) -> CassieError {
    memory_limit_error(error.budget, error.requested)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::{QueryCancellationHandle, QueryExecutionControls};
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
    fn should_keep_explain_observation_when_capturing_the_statement_read() {
        // Arrange
        let controls = controls_with_budget(1024);
        controls.with_statement_state(|slot| *slot = Some(Box::new(7_usize)));
        let explain = controls.observe_ordered_join();
        let independent = controls.observe_ordered_join();

        // Act
        let same_cache = explain.with_statement_state(|slot| {
            slot.as_ref()
                .and_then(|value| value.downcast_ref::<usize>())
                .copied()
        });
        let captured = explain.with_statement_read(None);
        let fresh_cache = captured.with_statement_state(|slot| slot.is_none());
        captured.record_order_proof(12, "canonical_timestamp_prefix");

        // Assert
        assert_eq!(same_cache, Some(7));
        assert!(fresh_cache);
        assert_eq!(
            explain.order_proof(),
            Some((12, "canonical_timestamp_prefix"))
        );
        assert_eq!(independent.order_proof(), None);
        assert_eq!(controls.order_proof(), None);
    }

    #[test]
    fn should_observe_cancellation_that_precedes_wait_registration() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();

        // Act
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(1), cancellation.wait_canceled())
                .await
                .expect("preexisting cancellation signal should wake the waiter");
        });

        // Assert
        assert!(cancellation.is_canceled());
    }

    #[test]
    fn should_wake_registered_waiter_when_cancellation_arrives() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let cancellation = QueryCancellationHandle::new();

        // Act
        runtime.block_on(async {
            let waiting = cancellation.clone();
            let mut wait = Box::pin(waiting.wait_canceled());
            tokio::select! {
                biased;
                () = wait.as_mut() => panic!("uncanceled waiter must remain pending"),
                () = tokio::task::yield_now() => {}
            }
            cancellation.cancel();
            tokio::time::timeout(Duration::from_secs(1), wait.as_mut())
                .await
                .expect("registered cancellation waiter should wake");
        });

        // Assert
        assert!(cancellation.is_canceled());
    }

    #[test]
    fn should_observe_query_memory_until_the_last_result_alias_is_dropped() {
        // Arrange
        let runtime = super::RuntimeState::new(CassieRuntimeLimits::default());
        let controls = controls_with_budget(1_048_576);
        let tracker = controls.memory.downgrade();
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
        let cancellation_owner = cancellation.0.downgrade();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let tracker = controls.memory.downgrade();
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
        let request_retired = !cancellation_owner.is_alive();
        let memory_live = tracker
            .upgrade()
            .expect("narrow retained tracker")
            .current_bytes();
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
