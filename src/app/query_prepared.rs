use std::time::Instant;

use super::{
    Cassie, CassieError, CassieSession, ExecutionMode, QueryCancellationHandle,
    QueryExecutionControls, QueryResult, Value,
};

impl Cassie {
    pub(crate) fn execute_parsed_sql_with_cancellation(
        &self,
        session: &CassieSession,
        statement: (crate::sql::ast::ParsedStatement, &str),
        sql_fingerprint: u64,
        parameters: (Vec<Value>, &[i32]),
        mode: ExecutionMode,
        cancellation: &QueryCancellationHandle,
    ) -> Result<QueryResult, CassieError> {
        self.execute_pgwire_query(session, cancellation, |controls| {
            let (parsed, sql) = statement;
            if let Some(result) =
                self.quarantined_statement_result(session, &parsed.statement, controls)
            {
                return result;
            }
            let execution_session = session.with_statement_text(sql, controls)?;
            self.execute_pgwire_statement_core(
                &execution_session,
                parsed,
                sql_fingerprint,
                parameters,
                mode,
                controls,
            )
        })
    }

    pub(crate) fn execute_pgwire_sql_with_cancellation(
        &self,
        session: &CassieSession,
        query: (&str, &str),
        cancellation: &QueryCancellationHandle,
    ) -> Result<QueryResult, CassieError> {
        self.execute_pgwire_query(session, cancellation, |controls| {
            let (sql, frontend_sql) = query;
            if let Some(result) = self.quarantined_sql_result(session, sql, controls) {
                return result;
            }
            let execution_session = session.with_statement_text(frontend_sql, controls)?;
            let session = &execution_session;
            if session.user.is_empty() {
                return Err(CassieError::Unauthorized);
            }
            self.ensure_session_database_access(session)?;
            if let Some(error) = super::unsupported_sql_error(sql) {
                if session.is_authenticated_read_only() {
                    return Err(CassieError::InsufficientPrivilege);
                }
                return Err(error);
            }
            if controls.is_cancelled() {
                return Err(CassieError::QueryCancelled);
            }
            if controls.is_timed_out() {
                return Err(CassieError::DeadlineExceeded);
            }
            self.runtime.record_sql_parse();
            let parsed = crate::sql::parser::parse_statement(sql)?;
            let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
            self.execute_pgwire_statement_core(
                session,
                parsed,
                sql_fingerprint,
                (Vec::new(), &[]),
                ExecutionMode::SimpleQuery,
                controls,
            )
        })
    }

    fn execute_pgwire_query(
        &self,
        session: &CassieSession,
        cancellation: &QueryCancellationHandle,
        execute: impl FnOnce(&QueryExecutionControls) -> Result<QueryResult, CassieError>,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let running_guard = if self.check_storage_recovery_required().is_err() {
            None
        } else if let Some(guard) = self.runtime.try_begin_running_query() {
            Some(guard)
        } else {
            match self.check_storage_recovery_required() {
                Err(_) => None,
                Ok(()) => {
                    return Err(CassieError::Execution(
                        "query admission exhausted".to_string(),
                    ))
                }
            }
        };
        let mut controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            query_started,
            cancellation.clone(),
        );
        session.apply_statement_timeout(&mut controls, query_started);
        self.runtime.record_query_memory(&controls);
        let result = execute(&controls);
        let elapsed = query_started.elapsed();
        self.runtime.record_query_memory(&controls);

        match &result {
            Ok(result) => self
                .runtime
                .record_query_success(elapsed, result.row_count()),
            Err(error) => {
                self.runtime.record_query_error(elapsed, error);
                if session.is_transaction_active() {
                    session.mark_transaction_failed();
                }
            }
        }

        drop(running_guard);
        if self.check_storage_recovery_required().is_ok() {
            let _ = self.run_deferred_schema_cleanup();
        }
        result
    }
}
