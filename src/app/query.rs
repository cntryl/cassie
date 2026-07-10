use super::query_explain::{
    QueryExplainOutput, QueryPlanAnalyze, QueryPlanAnalyzeDiagnostics, QueryPlanOperatorActual,
};
use super::{
    current_time_millis, parser, query_cache, unsupported_sql_error, Arc, Cassie, CassieError,
    CassieSession, ColumnMeta, ExecutionMode, Instant, PlanCacheKey, PlanCacheProvenance,
    QueryExecutionControls, QueryResult, QueryStatement, RuntimeFeedbackObservation,
    TransactionAction, TransactionStatement, Value,
};
use std::fmt::Write as _;

const PLAN_CACHE_COST_MODEL_VERSION: u32 = 2;

struct QueryCacheContext {
    is_select: bool,
    cache_key: Option<PlanCacheKey>,
    exec_cache_key: Option<crate::runtime::ExecutionResultCacheKey>,
}

struct QueryFeedbackCapture {
    keys: Option<Vec<crate::runtime::RuntimeFeedbackKey>>,
    before: Option<crate::runtime::RuntimeMetricsSnapshot>,
    started_at: Instant,
}

struct RuntimeFeedbackDeltas {
    storage_reads: u64,
    storage_writes: u64,
    temp_writes: u64,
    candidate_count: u64,
    result_count: u64,
}

impl RuntimeFeedbackDeltas {
    fn from_snapshots(
        before: &crate::runtime::RuntimeMetricsSnapshot,
        after: &crate::runtime::RuntimeMetricsSnapshot,
    ) -> Self {
        Self {
            storage_reads: after
                .storage
                .data
                .reads
                .saturating_sub(before.storage.data.reads),
            storage_writes: after
                .storage
                .data
                .writes
                .saturating_sub(before.storage.data.writes),
            temp_writes: after
                .storage
                .temp
                .writes
                .saturating_sub(before.storage.temp.writes),
            candidate_count: search_candidate_delta(before, after),
            result_count: search_result_delta(before, after),
        }
    }

    fn to_observation(
        &self,
        execution: &Result<QueryResult, CassieError>,
        elapsed_ms: u64,
    ) -> RuntimeFeedbackObservation {
        RuntimeFeedbackObservation {
            rows_in: self.storage_reads.saturating_add(self.candidate_count).max(
                execution
                    .as_ref()
                    .map_or(0, |result| result.rows.len() as u64),
            ),
            rows_out: execution
                .as_ref()
                .map_or(0, |result| result.rows.len() as u64),
            elapsed_ms,
            storage_reads: self.storage_reads,
            storage_writes: self.storage_writes,
            temp_writes: self.temp_writes,
            candidate_count: self.candidate_count,
            result_count: self.result_count,
            error_class: execution
                .as_ref()
                .err()
                .map(|error| crate::runtime::error_class(error).to_string()),
            spilled: self.temp_writes > 0,
            memory_pressure: self.temp_writes > 0,
        }
    }
}

struct ExplainAnalyzeReport {
    result: QueryResult,
    elapsed_ms: u128,
    deltas: ExplainAnalyzeDeltas,
}

struct ExplainAnalyzeDeltas {
    runtime: RuntimeFeedbackDeltas,
    plan_cache_hits: u64,
    plan_cache_misses: u64,
    parallel_aggregations: u64,
    parallel_aggregation_fallbacks: u64,
    parallel_aggregation_workers: u64,
    parallel_aggregation_groups: u64,
    adaptive_plan_decisions: u64,
    adaptive_plan_selected: u64,
    operator_switch_attempts: u64,
    operator_switch_successes: u64,
    operator_switch_skips: u64,
    operator_switch_fallbacks: u64,
}

impl ExplainAnalyzeDeltas {
    fn from_snapshots(
        before: &crate::runtime::RuntimeMetricsSnapshot,
        after: &crate::runtime::RuntimeMetricsSnapshot,
    ) -> Self {
        Self {
            runtime: RuntimeFeedbackDeltas::from_snapshots(before, after),
            plan_cache_hits: after.plan_cache.hits.saturating_sub(before.plan_cache.hits),
            plan_cache_misses: after
                .plan_cache
                .misses
                .saturating_sub(before.plan_cache.misses),
            parallel_aggregations: after
                .parallel_aggregation
                .aggregations
                .saturating_sub(before.parallel_aggregation.aggregations),
            parallel_aggregation_fallbacks: after
                .parallel_aggregation
                .fallback_aggregations
                .saturating_sub(before.parallel_aggregation.fallback_aggregations),
            parallel_aggregation_workers: after
                .parallel_aggregation
                .workers
                .saturating_sub(before.parallel_aggregation.workers),
            parallel_aggregation_groups: after
                .parallel_aggregation
                .groups
                .saturating_sub(before.parallel_aggregation.groups),
            adaptive_plan_decisions: after
                .adaptive_candidates
                .plan_decisions
                .saturating_sub(before.adaptive_candidates.plan_decisions),
            adaptive_plan_selected: after
                .adaptive_candidates
                .plan_selected_alternatives
                .saturating_sub(before.adaptive_candidates.plan_selected_alternatives),
            operator_switch_attempts: after
                .adaptive_candidates
                .operator_switch_attempts
                .saturating_sub(before.adaptive_candidates.operator_switch_attempts),
            operator_switch_successes: after
                .adaptive_candidates
                .operator_switch_successes
                .saturating_sub(before.adaptive_candidates.operator_switch_successes),
            operator_switch_skips: after
                .adaptive_candidates
                .operator_switch_skips
                .saturating_sub(before.adaptive_candidates.operator_switch_skips),
            operator_switch_fallbacks: after
                .adaptive_candidates
                .operator_switch_fallbacks
                .saturating_sub(before.adaptive_candidates.operator_switch_fallbacks),
        }
    }

    fn to_success_observation(
        &self,
        result: &QueryResult,
        elapsed_ms: u64,
    ) -> RuntimeFeedbackObservation {
        RuntimeFeedbackObservation {
            rows_in: self
                .runtime
                .storage_reads
                .saturating_add(self.runtime.candidate_count)
                .max(result.rows.len() as u64),
            rows_out: result.rows.len() as u64,
            elapsed_ms,
            storage_reads: self.runtime.storage_reads,
            storage_writes: self.runtime.storage_writes,
            temp_writes: self.runtime.temp_writes,
            candidate_count: self.runtime.candidate_count,
            result_count: self.runtime.result_count,
            error_class: None,
            spilled: self.runtime.temp_writes > 0,
            memory_pressure: self.runtime.temp_writes > 0,
        }
    }
}

fn search_candidate_delta(
    before: &crate::runtime::RuntimeMetricsSnapshot,
    after: &crate::runtime::RuntimeMetricsSnapshot,
) -> u64 {
    after
        .search
        .candidate_count_total
        .saturating_sub(before.search.candidate_count_total)
        .saturating_add(
            after
                .vector
                .candidate_count_total
                .saturating_sub(before.vector.candidate_count_total),
        )
        .saturating_add(
            after
                .hybrid
                .candidate_count_total
                .saturating_sub(before.hybrid.candidate_count_total),
        )
}

fn search_result_delta(
    before: &crate::runtime::RuntimeMetricsSnapshot,
    after: &crate::runtime::RuntimeMetricsSnapshot,
) -> u64 {
    after
        .search
        .result_count_total
        .saturating_sub(before.search.result_count_total)
        .saturating_add(
            after
                .vector
                .result_count_total
                .saturating_sub(before.vector.result_count_total),
        )
        .saturating_add(
            after
                .hybrid
                .result_count_total
                .saturating_sub(before.hybrid.result_count_total),
        )
}

impl Cassie {
    fn is_query_cacheable(statement: &QueryStatement) -> bool {
        matches!(statement, QueryStatement::Select(_))
    }

    fn plan_cache_key_from_fingerprint(
        &self,
        sql_fingerprint: u64,
        parameter_shape: Vec<crate::runtime::ParameterShape>,
        mode: ExecutionMode,
        database: Option<String>,
        search_path: &[String],
    ) -> PlanCacheKey {
        PlanCacheKey {
            sql_fingerprint,
            schema_epoch: self.runtime.schema_epoch(),
            data_epoch: self.runtime.data_epoch(),
            index_feedback_epoch: self.runtime.index_feedback_epoch(),
            cost_model_version: PLAN_CACHE_COST_MODEL_VERSION,
            adaptive_config_hash: self.adaptive_config_hash(),
            parameter_shape,
            mode,
            database,
            search_path: search_path.to_owned(),
        }
    }

    fn adaptive_config_hash(&self) -> u64 {
        let limits = self.runtime.limits();
        crate::runtime::stable_fingerprint(&(
            limits.adaptive_execution_enabled,
            limits.adaptive_min_cost_savings_bps,
            limits.adaptive_min_confidence_bps,
            limits.operator_feedback_enabled,
            limits.operator_switching_enabled.is_enabled(),
            limits.operator_switch_join_row_threshold,
        ))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn plan_cache_hit_for_diagnostics(
        &self,
        parsed: &crate::sql::ast::ParsedStatement,
        params: &[crate::types::Value],
        mode: ExecutionMode,
        database: Option<String>,
        search_path: &[String],
    ) -> bool {
        let key = self.plan_cache_key_from_fingerprint(
            crate::runtime::sql_fingerprint(parsed),
            crate::runtime::parameter_shape(params),
            mode,
            database,
            search_path,
        );
        self.runtime.plan_cache_lookup(&key).is_some()
    }

    fn plan_cache_provenance(
        hit: crate::runtime::L1PlanHit,
    ) -> (
        Arc<crate::planner::physical::PhysicalPlan>,
        PlanCacheProvenance,
    ) {
        (
            hit.plan,
            PlanCacheProvenance::L1 {
                durable: hit.durable,
                candidate_expires_at_ms: hit.candidate_expires_at_ms,
            },
        )
    }

    fn resolve_physical_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        key: PlanCacheKey,
        session: Option<&CassieSession>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<
        (
            Arc<crate::planner::physical::PhysicalPlan>,
            PlanCacheProvenance,
        ),
        CassieError,
    > {
        if let Some(hit) = self.runtime.plan_cache_lookup(&key) {
            return Ok(Self::plan_cache_provenance(hit));
        }

        if let Some(plan) = query_cache::lookup_plan(&self.midge, &self.runtime, &key)? {
            self.runtime.plan_cache_store(key, plan.clone(), true);
            return Ok((plan, PlanCacheProvenance::L2));
        }

        self.runtime.record_query_cache_compile_miss();
        let plan = self.compile_physical_plan(parsed, session, controls)?;
        self.runtime.plan_cache_store(key, plan.clone(), false);
        Ok((plan, PlanCacheProvenance::Compiled))
    }

    fn observe_query_plan_usage(
        &self,
        key: &PlanCacheKey,
        plan: &Arc<crate::planner::physical::PhysicalPlan>,
        provenance: &PlanCacheProvenance,
    ) -> Result<(), CassieError> {
        match provenance {
            PlanCacheProvenance::L2 | PlanCacheProvenance::L1 { durable: true, .. } => Ok(()),
            PlanCacheProvenance::L1 {
                durable: false,
                candidate_expires_at_ms,
            } => {
                let candidate_pending = candidate_expires_at_ms
                    .is_some_and(|expires_at_ms| current_time_millis() <= expires_at_ms);
                match query_cache::observe_non_durable_plan_usage(
                    &self.midge,
                    &self.runtime,
                    key,
                    plan,
                    candidate_pending,
                )? {
                    query_cache::NonDurablePlanOutcome::Durable => {
                        self.runtime.mark_plan_cache_entry_durable(key);
                    }
                    query_cache::NonDurablePlanOutcome::CandidatePending { ttl_seconds } => {
                        self.runtime
                            .mark_plan_cache_entry_candidate_pending(key, ttl_seconds);
                    }
                    query_cache::NonDurablePlanOutcome::Transient => {}
                }
                Ok(())
            }
            PlanCacheProvenance::Compiled => {
                match query_cache::observe_non_durable_plan_usage(
                    &self.midge,
                    &self.runtime,
                    key,
                    plan,
                    false,
                )? {
                    query_cache::NonDurablePlanOutcome::Durable => {
                        self.runtime.mark_plan_cache_entry_durable(key);
                    }
                    query_cache::NonDurablePlanOutcome::CandidatePending { ttl_seconds } => {
                        self.runtime
                            .mark_plan_cache_entry_candidate_pending(key, ttl_seconds);
                    }
                    query_cache::NonDurablePlanOutcome::Transient => {}
                }
                Ok(())
            }
        }
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn execute_sql(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_with_mode(session, sql, params, ExecutionMode::SimpleQuery)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn describe_sql(&self, sql: &str) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        if let Some(error) = unsupported_sql_error(sql) {
            return Err(error);
        }

        self.runtime.record_sql_parse();
        let parsed = parser::parse_statement(sql)?;
        let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.describe_parsed_statement(parsed, sql_fingerprint)
    }

    pub(crate) fn describe_parsed_statement(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        if matches!(parsed.statement, QueryStatement::Explain(_)) {
            return Ok(vec![ColumnMeta::text("QUERY PLAN")]);
        }
        if matches!(parsed.statement, QueryStatement::Transaction(_)) {
            return Ok(Vec::new());
        }

        let controls = self.runtime.query_controls(Instant::now());
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        let cache_key = Self::is_query_cacheable(&parsed.statement).then(|| {
            self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                Vec::new(),
                ExecutionMode::DescribeQuery,
                None,
                &[crate::catalog::DEFAULT_SCHEMA.to_string()],
            )
        });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, None, Some(&controls))?
        } else {
            (
                self.compile_physical_plan(parsed, None, Some(&controls))?,
                PlanCacheProvenance::Compiled,
            )
        };

        let user_functions = if crate::executor::plan_needs_user_functions(&physical.logical) {
            self.catalog
                .list_functions()
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<std::collections::HashMap<String, _>>()
        } else {
            std::collections::HashMap::new()
        };
        let collection_schema = self.catalog.get_schema(&physical.logical.collection);

        if let Some(command) = physical.logical.command.as_ref() {
            let returning = match command {
                crate::planner::logical::LogicalCommand::Insert(statement) => {
                    Some(statement.returning.as_slice())
                }
                crate::planner::logical::LogicalCommand::Update(statement) => {
                    Some(statement.returning.as_slice())
                }
                crate::planner::logical::LogicalCommand::Delete(statement) => {
                    Some(statement.returning.as_slice())
                }
                _ => None,
            };
            if let Some(returning) = returning {
                return Ok(crate::executor::columns_from_projection(
                    returning,
                    collection_schema.as_ref(),
                    &user_functions,
                ));
            }
            return Ok(Vec::new());
        }

        if let Some(key) = cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(crate::executor::columns_from_projection(
            &physical.logical.projection,
            collection_schema.as_ref(),
            &user_functions,
        ))
    }

    pub(crate) fn execute_sql_with_mode(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let running_guard = self.runtime.begin_running_query();
        let controls = self.runtime.query_controls(query_started);
        let result = self.execute_sql_core(session, sql, params, mode, &controls);
        let elapsed = query_started.elapsed();

        match &result {
            Ok(result) => self
                .runtime
                .record_query_success(elapsed, result.rows.len()),
            Err(error) => {
                self.runtime.record_query_error(elapsed, error);
                if session.is_transaction_active() {
                    session.mark_transaction_failed();
                }
            }
        }

        drop(running_guard);
        let _ = self.run_deferred_schema_cleanup();
        result
    }

    pub(crate) fn execute_sql_with_controls(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_core(session, sql, params, mode, controls)
    }

    pub(crate) fn explain_sql(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
    ) -> Result<QueryExplainOutput, CassieError> {
        let query_started = Instant::now();
        let running_guard = self.runtime.begin_running_query();
        let controls = self.runtime.query_controls(query_started);
        let result = self.explain_sql_core(session, sql, params, &controls);
        let elapsed = query_started.elapsed();

        match &result {
            Ok(output) => self
                .runtime
                .record_query_success(elapsed, output.result.rows.len()),
            Err(error) => {
                self.runtime.record_query_error(elapsed, error);
                if session.is_transaction_active() {
                    session.mark_transaction_failed();
                }
            }
        }

        drop(running_guard);
        let _ = self.run_deferred_schema_cleanup();
        result
    }

    fn explain_sql_core(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        controls: &QueryExecutionControls,
    ) -> Result<QueryExplainOutput, CassieError> {
        if session.user.is_empty() {
            return Err(CassieError::Unauthorized);
        }
        self.ensure_session_database_exists(session)?;

        if let Some(error) = unsupported_sql_error(sql) {
            return Err(error);
        }

        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        self.runtime.record_sql_parse();
        let parsed = parser::parse_statement(sql)?;
        let explain = if matches!(parsed.statement, QueryStatement::Explain(_)) {
            parsed
        } else {
            self.runtime.record_sql_parse();
            parser::parse_statement(&format!("EXPLAIN {}", sql.trim()))?
        };

        Self::ensure_statement_can_execute(session, &explain, controls)?;
        let QueryStatement::Explain(statement) = explain.statement else {
            return Err(CassieError::Execution(
                "expected explain statement after explain wrapping".to_string(),
            ));
        };

        self.explain_statement_output(
            session,
            *statement.statement,
            params,
            statement.analyze,
            controls,
        )
    }

    fn execute_sql_core(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        if session.user.is_empty() {
            return Err(CassieError::Unauthorized);
        }
        self.ensure_session_database_exists(session)?;

        if let Some(error) = unsupported_sql_error(sql) {
            return Err(error);
        }

        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        self.runtime.record_sql_parse();
        let parsed = parser::parse_statement(sql)?;
        let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.execute_parsed_statement_core(session, parsed, sql_fingerprint, params, mode, controls)
    }

    pub(crate) fn execute_parsed_statement_core(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        Self::ensure_statement_can_execute(session, &parsed, controls)?;
        if let QueryStatement::Explain(statement) = &parsed.statement {
            return self.explain_statement(
                session,
                statement.statement.as_ref().clone(),
                params,
                statement.analyze,
                controls,
            );
        }
        if let QueryStatement::Transaction(statement) = &parsed.statement {
            return self.execute_transaction_statement(session, statement);
        }

        let cache_context =
            self.query_cache_context(session, &parsed, sql_fingerprint, &params, mode);
        if let Some(cached) =
            self.try_execution_result_cache(&parsed, &cache_context, session, controls)?
        {
            return Ok(cached);
        }

        let (physical, provenance) =
            self.resolve_statement_plan(parsed, &cache_context, session, controls)?;
        self.record_select_plan_decision(cache_context.is_select, &physical);

        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        let feedback = self.capture_query_feedback(
            cache_context.is_select,
            session.database.as_deref(),
            &session.search_path(),
            &physical,
        );
        let execution = self.execute_physical_statement(session, &physical, params, controls);
        self.record_query_feedback(feedback, &execution);

        let result = execution?;

        Self::validate_result_limit(&result, controls)?;

        self.store_execution_result(&cache_context, &result);

        self.bump_data_epoch_for_command(&result);

        if let Some(key) = cache_context.cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(result)
    }

    fn ensure_statement_can_execute(
        session: &CassieSession,
        parsed: &crate::sql::ast::ParsedStatement,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        if session.is_transaction_failed() && !Self::is_transaction_recovery(parsed) {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
        Ok(())
    }

    fn is_transaction_recovery(parsed: &crate::sql::ast::ParsedStatement) -> bool {
        matches!(
            &parsed.statement,
            QueryStatement::Transaction(TransactionStatement {
                action: TransactionAction::Rollback | TransactionAction::RollbackTo { .. },
                ..
            })
        )
    }

    fn query_cache_context(
        &self,
        session: &CassieSession,
        parsed: &crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: &[crate::types::Value],
        mode: ExecutionMode,
    ) -> QueryCacheContext {
        let is_select = Self::is_query_cacheable(&parsed.statement);
        let cache_key = is_select.then(|| {
            self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                crate::runtime::parameter_shape(params),
                mode,
                session.database.clone(),
                &session.search_path(),
            )
        });
        let exec_cache_key = is_select.then(|| crate::runtime::ExecutionResultCacheKey {
            sql_fingerprint,
            params_hash: crate::runtime::hash_params(params),
            schema_epoch: self.runtime.schema_epoch(),
            data_epoch: self.runtime.data_epoch(),
            database: session.database.clone(),
            search_path: session.search_path(),
            mode,
        });
        QueryCacheContext {
            is_select,
            cache_key,
            exec_cache_key,
        }
    }

    fn try_execution_result_cache(
        &self,
        parsed: &crate::sql::ast::ParsedStatement,
        cache_context: &QueryCacheContext,
        session: &CassieSession,
        controls: &QueryExecutionControls,
    ) -> Result<Option<QueryResult>, CassieError> {
        let Some(exec_cache_key) = cache_context.exec_cache_key.as_ref() else {
            return Ok(None);
        };
        let Some(cached) = self.runtime.execution_result_cache_lookup(exec_cache_key) else {
            return Ok(None);
        };
        if let Some(key) = cache_context.cache_key.as_ref() {
            self.observe_cached_result_plan(parsed.clone(), key, session, controls)?;
        }
        Ok(Some(cached))
    }

    fn observe_cached_result_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        key: &PlanCacheKey,
        session: &CassieSession,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let (physical, provenance) = if let Some(hit) = self.runtime.plan_cache_lookup(key) {
            Self::plan_cache_provenance(hit)
        } else {
            self.resolve_physical_plan(parsed, key.clone(), Some(session), Some(controls))?
        };
        self.runtime
            .record_adaptive_plan_decision(&physical.adaptive_plan);
        self.observe_query_plan_usage(key, &physical, &provenance)
    }

    fn resolve_statement_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        cache_context: &QueryCacheContext,
        session: &CassieSession,
        controls: &QueryExecutionControls,
    ) -> Result<
        (
            Arc<crate::planner::physical::PhysicalPlan>,
            PlanCacheProvenance,
        ),
        CassieError,
    > {
        if let Some(key) = cache_context.cache_key.clone() {
            return self.resolve_physical_plan(parsed, key, Some(session), Some(controls));
        }
        Ok((
            self.compile_physical_plan(parsed, Some(session), Some(controls))?,
            PlanCacheProvenance::Compiled,
        ))
    }

    fn record_select_plan_decision(
        &self,
        is_select: bool,
        physical: &crate::planner::physical::PhysicalPlan,
    ) {
        if is_select {
            self.runtime
                .record_adaptive_plan_decision(&physical.adaptive_plan);
        }
    }

    fn capture_query_feedback(
        &self,
        is_select: bool,
        database: Option<&str>,
        search_path: &[String],
        physical: &crate::planner::physical::PhysicalPlan,
    ) -> QueryFeedbackCapture {
        let keys = is_select.then(|| {
            let keys = self.feedback_keys_for_plan(database, search_path, physical);
            self.observe_feedback_lookup(&keys);
            keys
        });
        let before = keys.as_ref().map(|_| self.runtime.snapshot());
        QueryFeedbackCapture {
            keys,
            before,
            started_at: Instant::now(),
        }
    }

    fn execute_physical_statement(
        &self,
        session: &CassieSession,
        physical: &Arc<crate::planner::physical::PhysicalPlan>,
        params: Vec<crate::types::Value>,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        crate::executor::run_with_session_controls(self, Some(session), physical, params, controls)
            .map_err(CassieError::from)
    }

    fn record_query_feedback(
        &self,
        capture: QueryFeedbackCapture,
        execution: &Result<QueryResult, CassieError>,
    ) {
        let Some(keys) = capture.keys else {
            return;
        };
        let after = self.runtime.snapshot();
        let before = capture.before.expect("feedback snapshot");
        let elapsed_ms = capture
            .started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let observation = RuntimeFeedbackDeltas::from_snapshots(&before, &after)
            .to_observation(execution, elapsed_ms);
        self.record_feedback_for_keys(keys, &observation);
    }

    fn validate_result_limit(
        result: &QueryResult,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if result.rows.len() <= controls.max_result_rows {
            return Ok(());
        }
        Err(CassieError::Execution(format!(
            "query result row limit exceeded: {} > {}",
            result.rows.len(),
            controls.max_result_rows
        )))
    }

    fn store_execution_result(&self, cache_context: &QueryCacheContext, result: &QueryResult) {
        if let Some(exec_cache_key) = cache_context.exec_cache_key.clone() {
            self.runtime
                .execution_result_cache_store(exec_cache_key, result.clone());
        }
    }

    fn bump_data_epoch_for_command(&self, result: &QueryResult) {
        let command = result.command.as_str();
        if command.starts_with("INSERT")
            || command.starts_with("UPDATE")
            || command.starts_with("DELETE")
        {
            self.runtime.bump_data_epoch();
        }
    }

    fn explain_statement(
        &self,
        session: &CassieSession,
        statement: crate::sql::ast::ParsedStatement,
        params: Vec<crate::types::Value>,
        analyze: bool,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        Ok(self
            .explain_statement_output(session, statement, params, analyze, controls)?
            .result)
    }

    fn explain_statement_output(
        &self,
        session: &CassieSession,
        statement: crate::sql::ast::ParsedStatement,
        params: Vec<crate::types::Value>,
        analyze: bool,
        controls: &QueryExecutionControls,
    ) -> Result<QueryExplainOutput, CassieError> {
        let before = analyze.then(|| self.runtime.snapshot());
        let physical = self.compile_physical_plan(statement, Some(session), Some(controls))?;
        let mut plan = super::query_explain::plan_line(self, &physical);
        let mut structured = super::query_explain::structured_plan(self, &physical);

        if analyze {
            let report = self.run_explain_analyze(
                session,
                &physical,
                params,
                controls,
                &before.expect("analyze snapshot"),
            )?;
            append_explain_analyze(&mut plan, &physical, &report);
            structured.analyze = Some(structured_analyze(&physical, &report));
        }

        let result = QueryResult {
            columns: vec![ColumnMeta::text("QUERY PLAN")],
            rows: vec![vec![Value::String(plan)]],
            command: "EXPLAIN".to_string(),
        };

        Ok(QueryExplainOutput {
            result,
            plan: structured,
        })
    }

    fn run_explain_analyze(
        &self,
        session: &CassieSession,
        physical: &Arc<crate::planner::physical::PhysicalPlan>,
        params: Vec<crate::types::Value>,
        controls: &QueryExecutionControls,
        before: &crate::runtime::RuntimeMetricsSnapshot,
    ) -> Result<ExplainAnalyzeReport, CassieError> {
        self.runtime
            .record_adaptive_plan_decision(&physical.adaptive_plan);
        let feedback_keys = self.feedback_keys_for_plan(
            session.database.as_deref(),
            &session.search_path(),
            physical,
        );
        self.observe_feedback_lookup(&feedback_keys);
        let started_at = Instant::now();
        let result = self.execute_physical_statement(session, physical, params, controls)?;
        let elapsed_ms = started_at.elapsed().as_millis();
        let after = self.runtime.snapshot();
        let deltas = ExplainAnalyzeDeltas::from_snapshots(before, &after);
        self.record_explain_feedback(feedback_keys, &result, &deltas, elapsed_ms);
        Ok(ExplainAnalyzeReport {
            result,
            elapsed_ms,
            deltas,
        })
    }

    fn record_explain_feedback(
        &self,
        feedback_keys: Vec<crate::runtime::RuntimeFeedbackKey>,
        result: &QueryResult,
        deltas: &ExplainAnalyzeDeltas,
        elapsed_ms: u128,
    ) {
        let elapsed_ms = elapsed_ms.try_into().unwrap_or(u64::MAX);
        self.record_feedback_for_keys(
            feedback_keys,
            &deltas.to_success_observation(result, elapsed_ms),
        );
    }
}

fn append_explain_analyze(
    plan: &mut String,
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) {
    let actual_operators = actual_operator_diagnostics(physical, report);
    let deltas = &report.deltas;
    let runtime = &deltas.runtime;
    let _ = write!(
        plan,
        " analyze=true actual_rows={} actual_ms={} operator_actuals={} diagnostics=plan_cache_hits_delta:{},plan_cache_misses_delta:{},storage_reads_delta:{},storage_writes_delta:{},temp_writes_delta:{},candidate_count_delta:{},result_count_delta:{},parallel_aggregations_delta:{},parallel_aggregation_fallback_delta:{},parallel_aggregation_workers_delta:{},parallel_aggregation_groups_delta:{},adaptive_plan_decisions_delta:{},adaptive_plan_selected_delta:{},operator_switch_attempts_delta:{},operator_switch_success_delta:{},operator_switch_skips_delta:{},operator_switch_fallbacks_delta:{}",
        report.result.rows.len(),
        report.elapsed_ms,
        actual_operators,
        deltas.plan_cache_hits,
        deltas.plan_cache_misses,
        runtime.storage_reads,
        runtime.storage_writes,
        runtime.temp_writes,
        runtime.candidate_count,
        runtime.result_count,
        deltas.parallel_aggregations,
        deltas.parallel_aggregation_fallbacks,
        deltas.parallel_aggregation_workers,
        deltas.parallel_aggregation_groups,
        deltas.adaptive_plan_decisions,
        deltas.adaptive_plan_selected,
        deltas.operator_switch_attempts,
        deltas.operator_switch_successes,
        deltas.operator_switch_skips,
        deltas.operator_switch_fallbacks
    );
}

fn actual_operator_diagnostics(
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> String {
    if physical.operators.is_empty() {
        return "Command".to_string();
    }
    physical
        .operators
        .iter()
        .map(|operator| {
            format!(
                "{operator:?}:rows_in:{} rows_out:{} elapsed_ms:{} storage_reads:{} storage_writes:{} temp_writes:{} candidates:{} results:{}",
                physical.estimates.scan_rows,
                report.result.rows.len(),
                report.elapsed_ms,
                report.deltas.runtime.storage_reads,
                report.deltas.runtime.storage_writes,
                report.deltas.runtime.temp_writes,
                report.deltas.runtime.candidate_count,
                report.deltas.runtime.result_count
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn structured_analyze(
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> QueryPlanAnalyze {
    QueryPlanAnalyze {
        actual_rows: report.result.rows.len(),
        actual_ms: report.elapsed_ms,
        operator_actuals: structured_operator_actuals(physical, report),
        diagnostics: structured_analyze_diagnostics(report),
    }
}

fn structured_operator_actuals(
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> Vec<QueryPlanOperatorActual> {
    if physical.operators.is_empty() {
        return vec![operator_actual("Command", physical, report)];
    }

    physical
        .operators
        .iter()
        .map(|operator| operator_actual(format!("{operator:?}"), physical, report))
        .collect()
}

fn operator_actual(
    operator: impl Into<String>,
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> QueryPlanOperatorActual {
    QueryPlanOperatorActual {
        operator: operator.into(),
        rows_in: physical.estimates.scan_rows,
        rows_out: report.result.rows.len(),
        elapsed_ms: report.elapsed_ms,
        storage_reads: report.deltas.runtime.storage_reads,
        storage_writes: report.deltas.runtime.storage_writes,
        temp_writes: report.deltas.runtime.temp_writes,
        candidates: report.deltas.runtime.candidate_count,
        results: report.deltas.runtime.result_count,
    }
}

fn structured_analyze_diagnostics(report: &ExplainAnalyzeReport) -> QueryPlanAnalyzeDiagnostics {
    QueryPlanAnalyzeDiagnostics {
        plan_cache_hits: report.deltas.plan_cache_hits,
        plan_cache_misses: report.deltas.plan_cache_misses,
        storage_reads: report.deltas.runtime.storage_reads,
        storage_writes: report.deltas.runtime.storage_writes,
        temp_writes: report.deltas.runtime.temp_writes,
        candidate_count: report.deltas.runtime.candidate_count,
        result_count: report.deltas.runtime.result_count,
        parallel_aggregations: report.deltas.parallel_aggregations,
        parallel_aggregation_fallback: report.deltas.parallel_aggregation_fallbacks,
        parallel_aggregation_workers: report.deltas.parallel_aggregation_workers,
        parallel_aggregation_groups: report.deltas.parallel_aggregation_groups,
        adaptive_plan_decisions: report.deltas.adaptive_plan_decisions,
        adaptive_plan_selected: report.deltas.adaptive_plan_selected,
        operator_switch_attempts: report.deltas.operator_switch_attempts,
        operator_switch_success: report.deltas.operator_switch_successes,
        operator_switch_skips: report.deltas.operator_switch_skips,
        operator_switch_fallbacks: report.deltas.operator_switch_fallbacks,
    }
}
