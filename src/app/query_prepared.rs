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
        params: Vec<Value>,
        mode: ExecutionMode,
        cancellation: &QueryCancellationHandle,
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
        let result = self.execute_parsed_statement_core(
            session,
            parsed,
            sql_fingerprint,
            params,
            mode,
            &controls,
        );
        let elapsed = query_started.elapsed();
        self.runtime
            .record_query_peak_memory(controls.peak_query_memory_bytes());

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
