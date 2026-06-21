use super::*;

const PLAN_CACHE_COST_MODEL_VERSION: u32 = 1;

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
    ) -> PlanCacheKey {
        PlanCacheKey {
            sql_fingerprint,
            schema_epoch: self.runtime.schema_epoch(),
            data_epoch: self.runtime.data_epoch(),
            index_feedback_epoch: self.runtime.index_feedback_epoch(),
            cost_model_version: PLAN_CACHE_COST_MODEL_VERSION,
            parameter_shape,
            mode,
            database,
        }
    }

    fn feedback_keys_for_plan(
        &self,
        sql_fingerprint: u64,
        database: Option<String>,
        physical: &crate::planner::physical::PhysicalPlan,
    ) -> Vec<RuntimeFeedbackKey> {
        let schema_epoch = self.runtime.schema_epoch();
        let collection = physical.collection.clone();
        let mut keys = physical
            .operators
            .iter()
            .map(|operator| RuntimeFeedbackKey {
                sql_fingerprint,
                schema_epoch,
                database: database.clone(),
                collection: collection.clone(),
                operator: format!("{operator:?}"),
            })
            .collect::<Vec<_>>();
        if let Some(index) = &physical.selected_index {
            keys.push(RuntimeFeedbackKey {
                sql_fingerprint,
                schema_epoch,
                database,
                collection,
                operator: format!("Index:{index}"),
            });
        }
        keys
    }

    fn observe_feedback_lookup(&self, keys: &[RuntimeFeedbackKey]) {
        for key in keys {
            let _ = self.runtime.feedback_lookup(key);
        }
    }

    fn record_feedback_for_keys(
        &self,
        keys: Vec<RuntimeFeedbackKey>,
        observation: RuntimeFeedbackObservation,
    ) {
        for key in keys {
            self.runtime.record_feedback(key, observation.clone());
        }
    }

    #[doc(hidden)]
    pub fn plan_cache_hit_for_diagnostics(
        &self,
        parsed: &crate::sql::ast::ParsedStatement,
        params: &[crate::types::Value],
        mode: ExecutionMode,
        database: Option<String>,
    ) -> bool {
        let key = self.plan_cache_key_from_fingerprint(
            crate::runtime::sql_fingerprint(parsed),
            crate::runtime::parameter_shape(params),
            mode,
            database,
        );
        self.runtime.plan_cache_lookup(&key).is_some()
    }

    #[doc(hidden)]
    pub fn feedback_record_for_diagnostics(
        &self,
        key: &RuntimeFeedbackKey,
    ) -> Option<crate::runtime::RuntimeFeedbackRecord> {
        self.runtime.feedback_record(key)
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
        let plan = self.compile_physical_plan(parsed, controls)?;
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
            PlanCacheProvenance::L2 => Ok(()),
            PlanCacheProvenance::L1 { durable: true, .. } => Ok(()),
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

    pub fn execute_sql(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_with_mode(session, sql, params, ExecutionMode::SimpleQuery)
    }

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
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let cache_key = Self::is_query_cacheable(&parsed.statement).then(|| {
            self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                Vec::new(),
                ExecutionMode::DescribeQuery,
                None,
            )
        });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, Some(&controls))?
        } else {
            (
                self.compile_physical_plan(parsed, Some(&controls))?,
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

    fn compile_physical_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Arc<crate::planner::physical::PhysicalPlan>, CassieError> {
        let bound = binder::bind(parsed, &self.catalog)?;
        if controls.is_some_and(QueryExecutionControls::is_timed_out) {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let logical = crate::planner::logical::plan(&bound)?;
        let optimized = crate::planner::optimizer::optimize(logical);
        let cardinality_stats = self.catalog.cardinality.read().clone();
        Ok(Arc::new(crate::planner::physical::build_with_indexes(
            optimized,
            bound.indexes,
            &cardinality_stats,
        )))
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

        if let Some(error) = unsupported_sql_error(sql) {
            return Err(error);
        }

        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        self.runtime.record_sql_parse();
        let parsed = parser::parse_statement(sql)?;
        let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.execute_parsed_statement_core(session, parsed, sql_fingerprint, params, mode, controls)
    }

    pub(crate) fn execute_preparsed_statement_with_mode(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let running_guard = self.runtime.begin_running_query();
        let controls = self.runtime.query_controls(query_started);
        let result = self.execute_parsed_statement_core(
            session,
            parsed,
            sql_fingerprint,
            params,
            mode,
            &controls,
        );
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
        result
    }

    fn execute_parsed_statement_core(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }
        if session.is_transaction_failed()
            && !matches!(
                &parsed.statement,
                QueryStatement::Transaction(TransactionStatement {
                    action: TransactionAction::Rollback | TransactionAction::RollbackTo { .. },
                    ..
                })
            )
        {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
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

        let is_select = Self::is_query_cacheable(&parsed.statement);
        let cache_key = is_select.then(|| {
            self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                crate::runtime::parameter_shape(&params),
                mode,
                session.database.clone(),
            )
        });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, Some(controls))?
        } else {
            (
                self.compile_physical_plan(parsed, Some(controls))?,
                PlanCacheProvenance::Compiled,
            )
        };
        let feedback_keys = is_select.then(|| {
            let keys =
                self.feedback_keys_for_plan(sql_fingerprint, session.database.clone(), &physical);
            self.observe_feedback_lookup(&keys);
            keys
        });

        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let params_hash = if is_select {
            Some(crate::runtime::hash_params(&params))
        } else {
            None
        };

        if is_select {
            let exec_cache_key = crate::runtime::ExecutionResultCacheKey {
                sql_fingerprint,
                params_hash: params_hash.unwrap(),
                schema_epoch: self.runtime.schema_epoch(),
                data_epoch: self.runtime.data_epoch(),
                database: session.database.clone(),
                mode,
            };
            if let Some(cached) = self.runtime.execution_result_cache_lookup(&exec_cache_key) {
                if let Some(key) = cache_key.as_ref() {
                    self.observe_query_plan_usage(key, &physical, &provenance)?;
                }
                return Ok(cached);
            }
        }

        let feedback_before = feedback_keys.as_ref().map(|_| self.runtime.snapshot());
        let feedback_started_at = Instant::now();
        let execution = crate::executor::run_with_session_controls(
            self,
            Some(session),
            physical.clone(),
            params,
            controls,
        )
        .map_err(CassieError::from);

        if let Some(keys) = feedback_keys.clone() {
            let after = self.runtime.snapshot();
            let before = feedback_before.expect("feedback snapshot");
            let observation = RuntimeFeedbackObservation {
                rows_in: physical.estimates.scan_rows,
                rows_out: execution
                    .as_ref()
                    .map(|result| result.rows.len() as u64)
                    .unwrap_or(0),
                elapsed_ms: feedback_started_at
                    .elapsed()
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
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
                candidate_count: after
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
                    ),
                result_count: after
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
                    ),
                error_class: execution
                    .as_ref()
                    .err()
                    .map(|error| crate::runtime::error_class(error).to_string()),
            };
            self.record_feedback_for_keys(keys, observation);
        }

        let result = execution?;

        if result.rows.len() > controls.max_result_rows {
            return Err(CassieError::Execution(format!(
                "query result row limit exceeded: {} > {}",
                result.rows.len(),
                controls.max_result_rows
            )));
        }

        if is_select {
            let exec_cache_key = crate::runtime::ExecutionResultCacheKey {
                sql_fingerprint,
                params_hash: params_hash.unwrap(),
                schema_epoch: self.runtime.schema_epoch(),
                data_epoch: self.runtime.data_epoch(),
                database: session.database.clone(),
                mode,
            };
            self.runtime
                .execution_result_cache_store(exec_cache_key, result.clone());
        }

        let command = result.command.as_str();
        if command.starts_with("INSERT")
            || command.starts_with("UPDATE")
            || command.starts_with("DELETE")
        {
            self.runtime.bump_data_epoch();
        }

        if let Some(key) = cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(result)
    }

    fn execute_transaction_statement(
        &self,
        session: &CassieSession,
        statement: &TransactionStatement,
    ) -> Result<QueryResult, CassieError> {
        let command = match &statement.action {
            TransactionAction::Begin => {
                session.begin_transaction(statement.isolation)?;
                "BEGIN"
            }
            TransactionAction::Commit => {
                if session.is_transaction_failed() {
                    return Err(CassieError::Execution(
                        "transaction is failed; rollback required".to_string(),
                    ));
                }
                let mut changed_collections = Vec::new();
                for (collection, writes) in session.transaction_writes() {
                    changed_collections.push(collection.clone());
                    for (id, change) in writes {
                        let result = match change {
                            TransactionRowChange::Upsert(payload) => self
                                .midge
                                .put_document(&collection, Some(id), payload)
                                .map(|_| ()),
                            TransactionRowChange::Delete => {
                                self.midge.delete_document(&collection, &id).map(|_| ())
                            }
                        };
                        if let Err(error) = result {
                            session.mark_transaction_failed();
                            return Err(error);
                        }
                        self.refresh_cardinality_stats(&collection)?;
                        self.refresh_projection_metadata(&collection)?;
                    }
                }
                let controls = self.runtime.query_controls(std::time::Instant::now());
                for collection in changed_collections {
                    crate::executor::refresh_rollups_for_source_external(
                        self,
                        &collection,
                        &controls,
                    )
                    .map_err(|error| CassieError::Execution(format!("{error:?}")))?;
                }
                session.commit_transaction();
                "COMMIT"
            }
            TransactionAction::Rollback => {
                session.rollback_transaction();
                "ROLLBACK"
            }
            TransactionAction::Savepoint { name } => {
                session.create_savepoint(name)?;
                "SAVEPOINT"
            }
            TransactionAction::RollbackTo { name } => {
                session.rollback_to_savepoint(name)?;
                "ROLLBACK"
            }
            TransactionAction::Release { name } => {
                session.release_savepoint(name)?;
                "RELEASE"
            }
        };

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: command.to_string(),
        })
    }

    fn explain_statement(
        &self,
        session: &CassieSession,
        statement: crate::sql::ast::ParsedStatement,
        params: Vec<crate::types::Value>,
        analyze: bool,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let sql_fingerprint = crate::runtime::sql_fingerprint(&statement);
        let before = analyze.then(|| self.runtime.snapshot());
        let physical = self.compile_physical_plan(statement, Some(controls))?;
        let mut plan = super::query_explain::plan_line(self, &physical);

        if analyze {
            let feedback_keys =
                self.feedback_keys_for_plan(sql_fingerprint, session.database.clone(), &physical);
            self.observe_feedback_lookup(&feedback_keys);
            let started_at = Instant::now();
            let result = crate::executor::run_with_session_controls(
                self,
                Some(session),
                physical.clone(),
                params,
                controls,
            )
            .map_err(CassieError::from)?;
            let elapsed_ms = started_at.elapsed().as_millis();
            let after = self.runtime.snapshot();
            let before = before.expect("analyze snapshot");
            let plan_cache_hits_delta =
                after.plan_cache.hits.saturating_sub(before.plan_cache.hits);
            let plan_cache_misses_delta = after
                .plan_cache
                .misses
                .saturating_sub(before.plan_cache.misses);
            let storage_reads_delta = after
                .storage
                .data
                .reads
                .saturating_sub(before.storage.data.reads);
            let storage_writes_delta = after
                .storage
                .data
                .writes
                .saturating_sub(before.storage.data.writes);
            let temp_writes_delta = after
                .storage
                .temp
                .writes
                .saturating_sub(before.storage.temp.writes);
            let candidate_count_delta = after
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
                );
            let result_count_delta = after
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
                );
            let parallel_aggregations_delta = after
                .parallel_aggregation
                .aggregations
                .saturating_sub(before.parallel_aggregation.aggregations);
            let parallel_aggregation_fallback_delta = after
                .parallel_aggregation
                .fallback_aggregations
                .saturating_sub(before.parallel_aggregation.fallback_aggregations);
            let parallel_aggregation_workers_delta = after
                .parallel_aggregation
                .workers
                .saturating_sub(before.parallel_aggregation.workers);
            let parallel_aggregation_groups_delta = after
                .parallel_aggregation
                .groups
                .saturating_sub(before.parallel_aggregation.groups);
            self.record_feedback_for_keys(
                feedback_keys,
                RuntimeFeedbackObservation {
                    rows_in: physical.estimates.scan_rows,
                    rows_out: result.rows.len() as u64,
                    elapsed_ms: elapsed_ms.min(u128::from(u64::MAX)) as u64,
                    storage_reads: storage_reads_delta,
                    storage_writes: storage_writes_delta,
                    temp_writes: temp_writes_delta,
                    candidate_count: candidate_count_delta,
                    result_count: result_count_delta,
                    error_class: None,
                },
            );
            let actual_operators = if physical.operators.is_empty() {
                "Command".to_string()
            } else {
                physical
                    .operators
                    .iter()
                    .map(|operator| {
                        format!("{operator:?}:rows_in:{} rows_out:{} elapsed_ms:{} storage_reads:{} storage_writes:{} temp_writes:{} candidates:{} results:{}",
                            physical.estimates.scan_rows,
                            result.rows.len(),
                            elapsed_ms,
                            storage_reads_delta,
                            storage_writes_delta,
                            temp_writes_delta,
                            candidate_count_delta,
                            result_count_delta
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            };
            plan.push_str(&format!(
                " analyze=true actual_rows={} actual_ms={} operator_actuals={} diagnostics=plan_cache_hits_delta:{},plan_cache_misses_delta:{},storage_reads_delta:{},storage_writes_delta:{},temp_writes_delta:{},candidate_count_delta:{},result_count_delta:{},parallel_aggregations_delta:{},parallel_aggregation_fallback_delta:{},parallel_aggregation_workers_delta:{},parallel_aggregation_groups_delta:{}",
                result.rows.len(),
                elapsed_ms,
                actual_operators,
                plan_cache_hits_delta,
                plan_cache_misses_delta,
                storage_reads_delta,
                storage_writes_delta,
                temp_writes_delta,
                candidate_count_delta,
                result_count_delta,
                parallel_aggregations_delta,
                parallel_aggregation_fallback_delta,
                parallel_aggregation_workers_delta,
                parallel_aggregation_groups_delta
            ));
        }

        Ok(QueryResult {
            columns: vec![ColumnMeta::text("QUERY PLAN")],
            rows: vec![vec![Value::String(plan)]],
            command: "EXPLAIN".to_string(),
        })
    }
}
