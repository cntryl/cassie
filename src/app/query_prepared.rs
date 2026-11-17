#[cfg(test)]
#[path = "query_prepared/authority_capture_tests.rs"]
mod authority_capture_tests;
#[cfg(test)]
#[path = "query_prepared/authority_test_hooks.rs"]
pub(super) mod authority_test_hooks;
#[cfg(test)]
#[path = "query_prepared/authority_tests.rs"]
mod authority_tests;

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
        self.execute_pgwire_query(session, cancellation, |controls, authority| {
            let operation_session = session.with_operation_failure_authority(authority);
            let session = &operation_session;
            let (parsed, sql) = statement;
            if let Some(result) =
                self.quarantined_statement_result(session, &parsed.statement, controls)
            {
                return result;
            }
            let execution_session = session.with_statement_text(sql, controls)?;
            self.execute_pgwire_statement_core_for_operation(
                &execution_session,
                parsed,
                sql_fingerprint,
                parameters,
                mode,
                (controls, authority),
            )
        })
    }

    pub(crate) fn execute_pgwire_sql_with_cancellation(
        &self,
        session: &CassieSession,
        query: (&str, &str),
        cancellation: &QueryCancellationHandle,
    ) -> Result<QueryResult, CassieError> {
        self.execute_pgwire_query(session, cancellation, |controls, authority| {
            let operation_session = session.with_operation_failure_authority(authority);
            let session = &operation_session;
            let (sql, frontend_sql) = query;
            if let Some(result) = self.quarantined_sql_result(session, sql, controls) {
                return result;
            }
            let execution_session = session.with_statement_text(frontend_sql, controls)?;
            #[cfg(test)]
            authority_test_hooks::after_statement_text(self, session);
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
            if controls.is_canceled() {
                return Err(CassieError::QueryCanceled);
            }
            if controls.is_timed_out() {
                return Err(CassieError::DeadlineExceeded);
            }
            self.runtime.record_sql_parse();
            let parsed = self.parse_statement_for_session(Some(session), sql)?;
            let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
            self.execute_pgwire_statement_core_for_operation(
                session,
                parsed,
                sql_fingerprint,
                (Vec::new(), &[]),
                ExecutionMode::SimpleQuery,
                (controls, authority),
            )
        })
    }

    fn execute_pgwire_query(
        &self,
        session: &CassieSession,
        cancellation: &QueryCancellationHandle,
        execute: impl FnOnce(
            &QueryExecutionControls,
            Option<&super::FailureAuthority>,
        ) -> Result<QueryResult, CassieError>,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        session.note_operation_worker_started();
        let recovery_required = self.check_storage_recovery_required().is_err();
        #[cfg(test)]
        if !recovery_required {
            authority_test_hooks::after_healthy_check(self, session);
        }
        let running_guard = if recovery_required {
            None
        } else if let Some(guard) = self.runtime.try_begin_running_query() {
            Some(guard)
        } else {
            match self.check_storage_recovery_required() {
                Err(_) => None,
                Ok(()) => {
                    session.reject_operation_admission();
                    return Err(CassieError::Execution(
                        "query admission exhausted".to_string(),
                    ));
                }
            }
        };
        let mut controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            query_started,
            cancellation.clone(),
        );
        session.apply_statement_timeout(&mut controls, query_started);
        #[cfg(test)]
        if running_guard.is_some() {
            authority_test_hooks::after_admission(self, session);
        }
        #[cfg(test)]
        let authority = Some({
            super::FailureAuthority::capture_for_operation_with_contention(session, || {
                authority_test_hooks::on_capture_contended(self, session, &mut controls);
            })
        });
        #[cfg(not(test))]
        let authority = Some(super::FailureAuthority::capture_for_operation(session));
        #[cfg(test)]
        if let Some(authority) = authority.as_ref() {
            super::query_lifecycle::authority_test_hooks::after_root_capture(
                self, session, authority,
            );
        }
        self.runtime.record_query_memory(&controls);
        let result = execute(&controls, authority.as_ref());
        #[cfg(test)]
        authority_test_hooks::after_worker_result(self, session, authority.as_ref(), &result);
        let elapsed = query_started.elapsed();
        self.runtime.record_query_memory(&controls);

        match &result {
            Ok(result) => self
                .runtime
                .record_query_success(elapsed, result.row_count()),
            Err(error) => {
                self.runtime.record_query_error(elapsed, error);
                if let Some(authority) = authority.as_ref() {
                    authority.mark_failed_if_current(session);
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
