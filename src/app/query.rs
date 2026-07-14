use super::query_explain::QueryExplainOutput;
use super::query_metrics::{
    append_explain_analyze, structured_analyze, ExplainAnalyzeDeltas, ExplainAnalyzeReport,
    RuntimeFeedbackDeltas,
};
use super::{
    current_time_millis, parser, query_cache, unsupported_sql_error, Arc, Cassie, CassieError,
    CassieSession, ColumnMeta, ExecutionMode, Instant, PlanCacheKey, PlanCacheProvenance,
    QueryExecutionControls, QueryResult, QueryStatement, TransactionAction, TransactionStatement,
    Value,
};
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

impl Cassie {
    fn is_query_cacheable(statement: &QueryStatement) -> bool {
        matches!(statement, QueryStatement::Select(_))
    }

    pub(super) fn plan_cache_key_from_fingerprint(
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

    pub(super) fn resolve_physical_plan(
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

    pub(super) fn observe_query_plan_usage(
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

    pub(crate) fn execute_sql_with_mode(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let Some(running_guard) = self.runtime.try_begin_running_query() else {
            return Err(CassieError::Execution(
                "query admission exhausted".to_string(),
            ));
        };
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
        let Some(running_guard) = self.runtime.try_begin_running_query() else {
            return Err(CassieError::Execution(
                "query admission exhausted".to_string(),
            ));
        };
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
            if session.is_authenticated_read_only() {
                return Err(CassieError::InsufficientPrivilege);
            }
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
            if session.is_authenticated_read_only() {
                return Err(CassieError::InsufficientPrivilege);
            }
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
        session.authorize_statement(&parsed.statement)?;
        if session.is_transaction_failed() && !Self::is_transaction_recovery(parsed) {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
        super::transaction_semantics::ensure_supported_transaction_semantics(
            session,
            &parsed.statement,
        )?;
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
