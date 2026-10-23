//! Bound query execution; wire output is validated before cache hits or writes.

use super::{
    Cassie, CassieError, CassieSession, ExecutionMode, QueryExecutionControls, QueryResult,
    QueryStatement, Value,
};

impl Cassie {
    pub(crate) fn execute_parsed_statement_core(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_parsed_statement_core_with_parameter_oids(
            session,
            parsed,
            sql_fingerprint,
            (params, &[]),
            mode,
            controls,
        )
    }

    pub(crate) fn execute_parsed_statement_core_with_parameter_oids(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameters: (Vec<crate::types::Value>, &[i32]),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_bound_statement(
            session,
            parsed,
            sql_fingerprint,
            (parameters.0, parameters.1, false),
            mode,
            controls,
        )
    }

    pub(super) fn execute_pgwire_statement_core(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameters: (Vec<Value>, &[i32]),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_bound_statement(
            session,
            parsed,
            sql_fingerprint,
            (parameters.0, parameters.1, true),
            mode,
            controls,
        )
    }

    pub(super) fn execute_bound_statement(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        if matches!(&parsed.statement, QueryStatement::Transaction(_)) {
            return self.execute_bound_statement_inner(
                session,
                parsed,
                sql_fingerprint,
                parameters,
                mode,
                controls,
            );
        }

        let _publication_read = self.catalog_publication.read();
        let transaction_active = session.is_transaction_active();
        let catalog_statement =
            parsed.statement.family() != crate::sql::ast::StatementFamily::Runtime;
        let catalog = if transaction_active {
            if catalog_statement && session.transaction_catalog_overlay().is_none() {
                session.begin_transaction_catalog_overlay(&self.catalog)
            } else {
                session
                    .transaction_catalog_overlay()
                    .unwrap_or_else(|| self.catalog.clone())
            }
        } else {
            self.catalog.clone()
        };
        let mut scoped_cassie = self.clone();
        scoped_cassie.catalog = catalog;
        let _storage_scope = if transaction_active {
            Some(self.midge.enter_transactional_ddl_scope(
                session.current_database().unwrap_or(&self.default_database),
                session.transaction_storage_overlay(),
                catalog_statement && !session.is_implicit_transaction(),
            )?)
        } else {
            None
        };
        scoped_cassie.execute_bound_statement_inner(
            session,
            parsed,
            sql_fingerprint,
            parameters,
            mode,
            controls,
        )
    }

    fn execute_bound_statement_inner(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        // A new top-level statement never borrows an enclosing read authority.
        // This also protects early transaction/EXPLAIN returns and fresh gated
        // commit validation when embedded execution is nested on one thread.
        let _entry_read_scope = crate::midge::adapter::StatementReadScope::enter(None);
        let _entry_overlay_scope = super::SessionReadScope::enter(None);
        self.ensure_session_database_access(session)?;
        self.ensure_statement_can_execute(session, &parsed, controls)?;
        Self::mark_statement_transaction_query(session, &parsed.statement);
        if let QueryStatement::SessionObject(statement) = &parsed.statement {
            let call = super::session_objects::SessionObjectCall {
                controls,
                mode,
                wire_output: parameters.2,
            };
            return self.execute_session_object(session, statement, &parsed.raw_sql, call);
        }
        self.execute_bound_query(session, parsed, sql_fingerprint, parameters, mode, controls)
    }

    /// Plans and runs one admitted statement inside the caller's read scopes.
    fn execute_bound_query(
        &self,
        session: &CassieSession,
        mut parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let (mut params, declared_oids, wire_output) = parameters;
        let (parameter_types, bound_parameters) =
            super::query_parameters::prepare_bound_parameters(
                self,
                session,
                &mut parsed,
                &mut params,
                declared_oids,
                controls,
            )?;
        let parameter_type_oids = parameter_types.types();
        if crate::sql::parser::is_concurrent_materialized_view_refresh(&parsed.raw_sql) {
            return self.execute_concurrent_materialized_view_refresh(session, parsed, controls);
        }
        if let QueryStatement::Explain(statement) = &parsed.statement {
            return self.execute_bound_explain(
                session,
                statement,
                params,
                parameter_type_oids,
                controls,
            );
        }
        if let QueryStatement::Transaction(statement) = &parsed.statement {
            return self.execute_bound_transaction(session, statement, controls);
        }

        let mut cache_context = self.query_cache_context_with_controls(
            session,
            &parsed,
            sql_fingerprint,
            &params,
            (mode, parameter_type_oids),
            controls,
        )?;
        Self::disable_query_cache_for_bound_execution(
            session,
            bound_parameters,
            &mut cache_context,
        );
        let (physical, provenance) = self.resolve_statement_plan(
            parsed,
            &cache_context,
            session,
            controls,
            parameter_type_oids,
        )?;
        let mut output_columns = self.validated_statement_output_columns(
            session,
            &physical.logical,
            (parameter_type_oids, declared_oids, wire_output),
            controls,
        )?;
        if !wire_output && parameter_types.is_admitted() && !parameter_type_oids.is_empty() {
            output_columns =
                Some(self.columns_for_plan(&physical.logical, Some(session), parameter_type_oids));
        }
        self.record_select_plan_decision(cache_context.is_select, &physical);

        let statement_controls =
            self.capture_bound_statement_read(session, &physical.logical, controls)?;
        let controls = &statement_controls;
        let (_read_scope, _overlay_scope) =
            Self::enter_bound_statement_read(&mut cache_context, controls);

        let result_cache_bypass = self.execution_result_cache_bypass_reason(session, &physical);
        if let Some(reason) = result_cache_bypass {
            self.runtime.record_execution_result_cache_bypass(reason);
        } else if let Some(mut cached) =
            self.try_execution_result_cache(&cache_context, controls)?
        {
            if let Some(key) = cache_context.cache_key.as_ref() {
                self.observe_query_plan_usage(key, &physical, &provenance)?;
            }
            if let Some(columns) = output_columns {
                cached = cached.try_with_columns(controls, &columns)?;
            }
            return Ok(cached);
        }

        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        let mut result = self.execute_bound_plan_with_feedback(
            session,
            &physical,
            params,
            cache_context.is_select,
            controls,
        )?;
        if let Some(columns) = output_columns {
            result = result.try_with_columns(controls, &columns)?;
        }

        Self::validate_result_limit(&result, controls)?;

        if result_cache_bypass.is_none() {
            self.store_execution_result(&cache_context, &result);
        }

        if let Some(key) = cache_context.cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(result)
    }

    fn execute_concurrent_materialized_view_refresh(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let binding_context = self.binding_context_for_session(Some(session));
        let bound = crate::sql::binder::bind_with_context(parsed, &self.catalog, &binding_context)?;
        let QueryStatement::RefreshMaterializedView(statement) = bound.statement.statement else {
            return Err(CassieError::Execution(
                "expected concurrent materialized-view refresh after binding".to_string(),
            ));
        };
        let user_functions = self.user_functions_for_session(Some(session));
        let result = crate::executor::refresh_materialized_view_concurrently_external(
            self,
            &statement,
            &user_functions,
            controls,
        )?;
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(result)
    }

    fn execute_bound_explain(
        &self,
        session: &CassieSession,
        statement: &crate::sql::ast::ExplainStatement,
        params: Vec<Value>,
        parameter_type_oids: &[i32],
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.explain_statement(
            session,
            statement.statement.as_ref().clone(),
            (params, parameter_type_oids),
            statement.analyze,
            controls,
        )
    }

    fn execute_bound_transaction(
        &self,
        session: &CassieSession,
        statement: &crate::sql::ast::TransactionStatement,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_transaction_statement(session, statement, controls)
    }

    fn disable_query_cache_for_bound_execution(
        session: &CassieSession,
        bound_parameters: bool,
        cache_context: &mut super::query::QueryCacheContext,
    ) {
        if session.transaction_catalog_overlay().is_some() || bound_parameters {
            cache_context.cache_key = None;
        }
    }

    fn mark_statement_transaction_query(session: &CassieSession, statement: &QueryStatement) {
        if !matches!(
            statement,
            QueryStatement::Set(_) | QueryStatement::Show(_) | QueryStatement::Transaction(_)
        ) {
            session.mark_transaction_query();
        }
    }

    fn execute_bound_plan_with_feedback(
        &self,
        session: &CassieSession,
        physical: &std::sync::Arc<crate::planner::physical::PhysicalPlan>,
        params: Vec<Value>,
        is_select: bool,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let feedback = self.capture_query_feedback(
            is_select,
            session.database.as_deref(),
            &session.search_path(),
            physical,
        );
        let execution = self.execute_physical_statement(session, physical, params, controls);
        self.record_query_feedback(feedback, &execution);
        execution
    }

    fn enter_bound_statement_read(
        cache_context: &mut super::query::QueryCacheContext,
        controls: &QueryExecutionControls,
    ) -> (
        crate::midge::adapter::StatementReadScope,
        super::SessionReadScope,
    ) {
        if let (Some(key), Some(owner)) = (
            cache_context.exec_cache_key.as_mut(),
            controls.statement_read(),
        ) {
            key.data_epoch = owner.data_epoch();
        }
        let read_scope =
            crate::midge::adapter::StatementReadScope::enter(controls.statement_read());

        let overlay_scope = super::SessionReadScope::enter(
            controls.statement_read().and_then(|owner| owner.overlay()),
        );

        #[cfg(test)]
        super::statement_read_tests::after_statement_view_capture();

        (read_scope, overlay_scope)
    }

    fn capture_bound_statement_read(
        &self,
        session: &CassieSession,
        plan: &crate::planner::logical::LogicalPlan,
        controls: &QueryExecutionControls,
    ) -> Result<QueryExecutionControls, CassieError> {
        #[cfg(test)]
        super::statement_read_tests::before_statement_view_capture();
        let owner = if plan.command.is_none() && crate::executor::plan_needs_statement_data(plan) {
            Some(
                session.capture_statement_read(
                    &self.midge,
                    session
                        .database
                        .as_deref()
                        .unwrap_or(&self.default_database),
                    controls,
                )?,
            )
        } else {
            None
        };
        // Each top-level execution owns a fresh local clone. A nested writer
        // suspends a reader scope rather than borrowing stale validation state.
        Ok(controls.with_statement_read(owner))
    }

    fn validated_statement_output_columns(
        &self,
        session: &CassieSession,
        logical: &crate::planner::logical::LogicalPlan,
        wire: (&[i32], &[i32], bool),
        controls: &QueryExecutionControls,
    ) -> Result<Option<Vec<crate::executor::ColumnMeta>>, CassieError> {
        let (parameter_type_oids, declared_oids, wire_output) = wire;
        super::query_parameters::validate_plan_parameters(
            logical,
            &self.catalog,
            &self.binding_context_for_session(Some(session)),
            parameter_type_oids,
        )?;
        self.bound_statement_output_columns(logical, session, declared_oids, controls, wire_output)
    }
}
