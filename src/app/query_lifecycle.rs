//! Keep conversion and response preparation inside the originating query lifecycle.

use std::time::Instant;

use crate::executor::QueryResult;
use crate::runtime::{ExecutionMode, QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;

use super::{Cassie, CassieError, CassieSession, QueryExplainOutput};

impl Cassie {
    pub(super) fn execute_sql_with_mode_and_cancellation(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        mode: ExecutionMode,
        cancellation: QueryCancellationHandle,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_converted(session, sql, params, (mode, cancellation), |result, _| {
            Ok(result)
        })
    }

    pub(crate) fn execute_sql_with_cancellation_converted<T>(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        cancellation: &QueryCancellationHandle,
        convert: impl FnOnce(QueryResult, &QueryExecutionControls) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.execute_sql_converted(
            session,
            sql,
            params,
            (ExecutionMode::SimpleQuery, cancellation.clone()),
            convert,
        )
    }

    fn execute_sql_converted<T>(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        execution: (ExecutionMode, QueryCancellationHandle),
        convert: impl FnOnce(QueryResult, &QueryExecutionControls) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let started = Instant::now();
        let Some(running) = self.runtime.try_begin_running_query() else {
            return Err(CassieError::Execution(
                "query admission exhausted".to_owned(),
            ));
        };
        let controls =
            QueryExecutionControls::with_cancellation(&self.runtime.limits(), started, execution.1);
        let mut rows = 0;
        let result = self
            .execute_sql_core(session, sql, params, execution.0, &controls)
            .and_then(|result| {
                rows = result.row_count();
                convert(result, &controls)
            });
        self.finish_converted_query(session, started, &controls, &result, rows);
        drop(running);
        let _ = self.run_deferred_schema_cleanup();
        result
    }

    pub(crate) fn explain_sql_with_cancellation_converted<T>(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        cancellation: &QueryCancellationHandle,
        convert: impl FnOnce(QueryExplainOutput, &QueryExecutionControls) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let started = Instant::now();
        let Some(running) = self.runtime.try_begin_running_query() else {
            return Err(CassieError::Execution(
                "query admission exhausted".to_owned(),
            ));
        };
        let controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            started,
            cancellation.clone(),
        );
        let mut rows = 0;
        let result = self
            .explain_sql_core(session, sql, params, &controls)
            .and_then(|output| {
                rows = output.result().row_count();
                convert(output, &controls)
            });
        self.finish_converted_query(session, started, &controls, &result, rows);
        drop(running);
        let _ = self.run_deferred_schema_cleanup();
        result
    }

    fn finish_converted_query<T>(
        &self,
        session: &CassieSession,
        started: Instant,
        controls: &QueryExecutionControls,
        result: &Result<T, CassieError>,
        rows: usize,
    ) {
        self.runtime.record_query_memory(controls);
        match result {
            Ok(_) => self.runtime.record_query_success(started.elapsed(), rows),
            Err(error) => {
                self.runtime.record_query_error(started.elapsed(), error);
                if session.is_transaction_active() {
                    session.mark_transaction_failed();
                }
            }
        }
    }
}
