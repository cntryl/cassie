use std::time::Instant;

use super::{Cassie, CassieError, CassieSession, ExecutionMode, QueryResult, Value};

impl Cassie {
    pub(crate) fn execute_parsed_sql_with_mode(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let Some(running_guard) = self.runtime.try_begin_running_query() else {
            return Err(CassieError::Execution(
                "query admission exhausted".to_string(),
            ));
        };
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
        let _ = self.run_deferred_schema_cleanup();
        result
    }
}
