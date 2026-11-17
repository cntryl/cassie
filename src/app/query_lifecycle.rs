//! Keep conversion and response preparation inside the originating query lifecycle.

#[cfg(test)]
pub(crate) mod authority_test_hooks;

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

    pub(crate) fn execute_copy_where_with_controls(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_copy_where_inner(session, sql, params, controls, None)
    }

    pub(crate) fn execute_copy_where_with_authority(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        controls: &QueryExecutionControls,
        authority: &super::FailureAuthority,
    ) -> Result<QueryResult, CassieError> {
        self.execute_copy_where_inner(session, sql, params, controls, Some(authority))
    }

    fn execute_copy_where_inner(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<Value>,
        controls: &QueryExecutionControls,
        authority: Option<&super::FailureAuthority>,
    ) -> Result<QueryResult, CassieError> {
        let operation_session = session.with_operation_failure_authority(authority);
        let session = &operation_session;
        let started = Instant::now();
        session.note_operation_worker_started();
        let _scope = crate::midge::adapter::PublicationControlScope::enter(Some(controls));
        if let Some((result, authority)) =
            self.quarantined_sql_result_with_authority(session, sql, controls)
        {
            let rows = result.as_ref().map_or(0, QueryResult::row_count);
            self.finish_converted_query(
                session,
                started,
                controls,
                &result,
                rows,
                Some(&authority),
            );
            return result;
        }
        let Some(running) = self.runtime.try_begin_running_query() else {
            session.reject_operation_admission();
            return Err(CassieError::Execution(
                "query admission exhausted".to_owned(),
            ));
        };
        let local_authority = authority
            .is_none()
            .then(|| super::FailureAuthority::capture_for_operation(session));
        let authority = authority
            .or(local_authority.as_ref())
            .expect("COPY failure authority");
        self.runtime.record_query_memory(controls);
        let result = self.execute_sql_core_for_operation(
            session,
            sql,
            params,
            ExecutionMode::SimpleQuery,
            (controls, Some(authority)),
        );
        let rows = result.as_ref().map_or(0, QueryResult::row_count);
        self.finish_converted_query(session, started, controls, &result, rows, Some(authority));
        drop(running);
        if self.check_storage_recovery_required().is_ok() {
            let _ = self.run_deferred_schema_cleanup();
        }
        result
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
        session.note_operation_worker_started();
        let mut controls =
            QueryExecutionControls::with_cancellation(&self.runtime.limits(), started, execution.1);
        session.apply_statement_timeout(&mut controls, started);
        let quarantine = self.quarantined_sql_result_with_authority(session, sql, &controls);
        #[cfg(test)]
        if quarantine.is_none() {
            authority_test_hooks::after_healthy_recovery_check(self, session);
        }
        if let Some((result, authority)) = quarantine {
            let mut rows = 0;
            let result = result.and_then(|result| {
                rows = result.row_count();
                convert(result, &controls)
            });
            self.finish_converted_query(
                session,
                started,
                &controls,
                &result,
                rows,
                Some(&authority),
            );
            return result;
        }
        let Some(running) = self.runtime.try_begin_running_query() else {
            session.reject_operation_admission();
            return Err(CassieError::Execution(
                "query admission exhausted".to_owned(),
            ));
        };
        #[cfg(test)]
        authority_test_hooks::after_admission_before_capture(self, session);
        #[cfg(test)]
        let authority =
            super::FailureAuthority::capture_for_operation_with_contention(session, || {
                authority_test_hooks::on_capture_contended(self, session, &mut controls);
            });
        #[cfg(not(test))]
        let authority = super::FailureAuthority::capture_for_operation(session);
        #[cfg(test)]
        authority_test_hooks::after_root_capture(self, session, &authority);
        self.runtime.record_query_memory(&controls);
        let mut rows = 0;
        let result = CassieSession::ensure_admitted_operation_owner(&authority)
            .and_then(|()| {
                self.execute_sql_core_for_operation(
                    session,
                    sql,
                    params,
                    execution.0,
                    (&controls, Some(&authority)),
                )
            })
            .and_then(|result| {
                rows = result.row_count();
                convert(result, &controls)
            });
        self.finish_converted_query(session, started, &controls, &result, rows, Some(&authority));
        drop(running);
        if self.check_storage_recovery_required().is_ok() {
            let _ = self.run_deferred_schema_cleanup();
        }
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
        self.check_storage_recovery_required()?;
        let started = Instant::now();
        let Some(running) = self.runtime.try_begin_running_query() else {
            session.reject_operation_admission();
            return Err(CassieError::Execution(
                "query admission exhausted".to_owned(),
            ));
        };
        let mut controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            started,
            cancellation.clone(),
        );
        session.apply_statement_timeout(&mut controls, started);
        #[cfg(test)]
        let authority =
            super::FailureAuthority::capture_for_operation_with_contention(session, || {
                authority_test_hooks::on_capture_contended(self, session, &mut controls);
            });
        #[cfg(not(test))]
        let authority = super::FailureAuthority::capture_for_operation(session);
        self.runtime.record_query_memory(&controls);
        let mut rows = 0;
        let result = CassieSession::ensure_admitted_operation_owner(&authority)
            .and_then(|()| {
                self.explain_sql_core_for_operation(
                    session,
                    sql,
                    params,
                    (&controls, Some(&authority)),
                )
            })
            .and_then(|output| {
                rows = output.result().row_count();
                convert(output, &controls)
            });
        self.finish_converted_query(session, started, &controls, &result, rows, Some(&authority));
        drop(running);
        if self.check_storage_recovery_required().is_ok() {
            let _ = self.run_deferred_schema_cleanup();
        }
        result
    }

    fn finish_converted_query<T>(
        &self,
        session: &CassieSession,
        started: Instant,
        controls: &QueryExecutionControls,
        result: &Result<T, CassieError>,
        rows: usize,
        authority: Option<&super::FailureAuthority>,
    ) {
        self.runtime.record_query_memory(controls);
        match result {
            Ok(_) => self.runtime.record_query_success(started.elapsed(), rows),
            Err(error) => {
                self.runtime.record_query_error(started.elapsed(), error);
                if let Some(authority) = authority {
                    authority.mark_failed_if_current(session);
                }
            }
        }
    }
}
