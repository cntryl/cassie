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

    fn execute_bound_statement(
        &self,
        session: &CassieSession,
        mut parsed: crate::sql::ast::ParsedStatement,
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
        let (mut params, declared_oids, wire_output) = parameters;
        self.ensure_session_database_access(session)?;
        Self::ensure_statement_can_execute(session, &parsed, controls)?;
        let parameter_type_oids =
            super::query_parameters::effective_parameter_type_oids(&params, declared_oids);
        let parameter_type_oids = parameter_type_oids.as_slice();
        super::query_parameters::canonicalize_string_parameters(
            &parsed,
            &self.catalog,
            &mut params,
        );
        let bound_parameters = crate::sql::pagination::resolve_statement(
            &mut parsed,
            &params,
            declared_oids,
            controls,
        )?;
        let is_data_read = matches!(&parsed.statement, QueryStatement::Select(_));
        if let QueryStatement::Explain(statement) = &parsed.statement {
            return self.explain_statement(
                session,
                statement.statement.as_ref().clone(),
                (params, parameter_type_oids),
                statement.analyze,
                controls,
            );
        }
        if let QueryStatement::Transaction(statement) = &parsed.statement {
            return self.execute_transaction_statement(session, statement);
        }

        let mut cache_context = self.query_cache_context(
            session,
            &parsed,
            sql_fingerprint,
            &params,
            mode,
            parameter_type_oids,
        );
        if bound_parameters {
            // Physical access paths depend on actual bounds, while the plan key
            // deliberately contains parameter shapes rather than values.
            cache_context.cache_key = None;
        }
        let (physical, provenance) = self.resolve_statement_plan(
            parsed,
            &cache_context,
            session,
            controls,
            parameter_type_oids,
        )?;
        super::query_parameters::validate_plan_parameters(
            &physical.logical,
            &self.catalog,
            &self.binding_context_for_session(Some(session)),
            parameter_type_oids,
        )?;
        let output_columns = self.bound_statement_output_columns(
            &physical.logical,
            session,
            declared_oids,
            controls,
            wire_output,
        )?;
        self.record_select_plan_decision(cache_context.is_select, &physical);

        let owner = if is_data_read {
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
        // suspends a reader scope rather than borrowing its stale validation view.
        let statement_controls = controls.with_statement_read(owner);
        let controls = &statement_controls;
        let _read_scope =
            crate::midge::adapter::StatementReadScope::enter(controls.statement_read());

        let _overlay_scope = super::SessionReadScope::enter(
            controls.statement_read().and_then(|owner| owner.overlay()),
        );

        #[cfg(test)]
        super::statement_read_tests::after_statement_view_capture();

        let result_cache_bypass = self.execution_result_cache_bypass_reason(session, &physical);
        if let Some(reason) = result_cache_bypass {
            self.runtime.record_execution_result_cache_bypass(reason);
        } else if let Some(mut cached) = self.try_execution_result_cache(&cache_context) {
            if let Some(key) = cache_context.cache_key.as_ref() {
                self.observe_query_plan_usage(key, &physical, &provenance)?;
            }
            if let Some(columns) = output_columns {
                cached.columns = columns;
            }
            return Ok(cached);
        }

        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
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

        let mut result = execution?;
        if let Some(columns) = output_columns {
            result.columns = columns;
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
}
