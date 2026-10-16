//! Validate executable plans at Bind without reading rows or applying writes.
use std::time::Instant;

use super::{Cassie, CassieError, CassieSession};
use crate::runtime::{ExecutionMode, QueryCancellationHandle, QueryExecutionControls};
use crate::sql::{ParsedStatement, QueryStatement};

impl Cassie {
    pub(crate) fn validate_pgwire_bind(
        &self,
        session: &CassieSession,
        parsed: ParsedStatement,
        sql_fingerprint: u64,
        parameter_types: &[i32],
        parameters: &[crate::types::Value],
        cancellation: &QueryCancellationHandle,
    ) -> Result<(), CassieError> {
        let controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            Instant::now(),
            cancellation.clone(),
        );
        let result = self.validate_pgwire_bind_with_controls(
            session,
            parsed,
            sql_fingerprint,
            parameter_types,
            parameters,
            &controls,
        );
        self.runtime.record_query_memory(&controls);
        result
    }

    fn validate_pgwire_bind_with_controls(
        &self,
        session: &CassieSession,
        parsed: ParsedStatement,
        sql_fingerprint: u64,
        parameter_types: &[i32],
        parameters: &[crate::types::Value],
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        self.ensure_session_database_access(session)?;
        Self::ensure_statement_can_execute(session, &parsed, controls)?;
        // Utility statements, notably EXPLAIN, plan their contained query when
        // executed. Bind must neither perform that work nor execute commands.
        if !matches!(
            parsed.statement,
            QueryStatement::Select(_)
                | QueryStatement::Insert(_)
                | QueryStatement::Update(_)
                | QueryStatement::Delete(_)
        ) {
            return Ok(());
        }
        let physical = if matches!(parsed.statement, QueryStatement::Select(_)) {
            let key = self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                super::query_parameters::parameter_shape_for_oids(parameter_types),
                ExecutionMode::ExtendedQuery,
                session.database.clone(),
                &session.search_path(),
            );
            self.resolve_physical_plan_with_parameter_oids(
                parsed,
                key,
                Some(session),
                Some(controls),
                parameter_types,
            )?
            .0
        } else {
            self.compile_physical_plan_with_parameter_oids(
                parsed,
                Some(session),
                Some(controls),
                parameter_types,
            )?
        };
        crate::executor::check_bound_plan_constants(&physical.logical, controls, parameters)?;
        controls.check_interruption()
    }
}

#[cfg(test)]
#[path = "query_bind_tests.rs"]
mod tests;
