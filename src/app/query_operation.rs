//! Explicit operation authority at SQL and executor boundaries.

use super::{
    Cassie, CassieError, CassieSession, ExecutionMode, QueryExecutionControls, QueryResult,
};
use std::sync::Arc;

impl Cassie {
    pub(crate) fn execute_sql_with_controls_for_operation(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        scope: (&QueryExecutionControls, Option<&super::FailureAuthority>),
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_core_for_operation(session, sql, params, mode, scope)
    }

    pub(super) fn execute_physical_statement_with_parameter_oids_for_operation(
        &self,
        session: &CassieSession,
        physical: &Arc<crate::planner::physical::PhysicalPlan>,
        params: Vec<crate::types::Value>,
        parameter_type_oids: &[i32],
        scope: (&QueryExecutionControls, Option<&super::FailureAuthority>),
    ) -> Result<QueryResult, CassieError> {
        crate::executor::run_with_session_parameter_oids_for_operation(
            self,
            Some(session),
            physical,
            params,
            parameter_type_oids,
            scope,
        )
        .map_err(CassieError::from)
    }
}
