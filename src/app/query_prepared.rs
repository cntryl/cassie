use std::time::Instant;

use super::{
    Cassie, CassieError, CassieSession, ExecutionMode, QueryCancellationHandle,
    QueryExecutionControls, QueryResult, Value,
};

impl Cassie {
    pub(crate) fn execute_parsed_sql_with_cancellation(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameters: (Vec<Value>, &[i32]),
        mode: ExecutionMode,
        cancellation: &QueryCancellationHandle,
    ) -> Result<QueryResult, CassieError> {
        self.execute_pgwire_query(session, cancellation, |controls| {
            self.execute_pgwire_statement_core(
                session,
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
        sql: &str,
        cancellation: &QueryCancellationHandle,
    ) -> Result<QueryResult, CassieError> {
        self.execute_pgwire_query(session, cancellation, |controls| {
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
        let Some(running_guard) = self.runtime.try_begin_running_query() else {
            return Err(CassieError::Execution(
                "query admission exhausted".to_string(),
            ));
        };
        let controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            query_started,
            cancellation.clone(),
        );
        let result = execute(&controls);
        let elapsed = query_started.elapsed();
        self.runtime.record_query_memory(&controls);

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
}
