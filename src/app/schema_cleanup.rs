use super::{Cassie, CassieError, CassieSession, QueryResult};

impl Cassie {
    pub(crate) fn run_deferred_schema_cleanup(&self) -> Result<(), CassieError> {
        for cleanup in self.midge.pending_schema_cleanups()? {
            if self
                .runtime
                .has_active_schema_epoch_at_or_before(cleanup.blocked_by_epoch)
            {
                continue;
            }
            self.midge.complete_pending_schema_cleanup(&cleanup)?;
        }
        Ok(())
    }

    #[doc(hidden)]
    #[must_use]
    pub fn begin_schema_epoch_guard_for_diagnostics(&self) -> crate::runtime::RunningQueryGuard {
        self.runtime.begin_running_query()
    }

    #[doc(hidden)]
    pub fn run_deferred_schema_cleanup_for_diagnostics(&self) -> Result<(), CassieError> {
        self.run_deferred_schema_cleanup()
    }

    #[doc(hidden)]
    pub fn compile_sql_physical_plan_for_diagnostics(
        &self,
        sql: &str,
    ) -> Result<std::sync::Arc<crate::planner::physical::PhysicalPlan>, CassieError> {
        let parsed = crate::sql::parser::parse_statement(sql)?;
        self.compile_physical_plan(parsed, None, None)
    }

    #[doc(hidden)]
    pub fn execute_physical_plan_for_diagnostics(
        &self,
        session: &CassieSession,
        plan: std::sync::Arc<crate::planner::physical::PhysicalPlan>,
    ) -> Result<QueryResult, CassieError> {
        let controls = self.runtime.query_controls(std::time::Instant::now());
        crate::executor::run_with_session_controls(
            self,
            Some(session),
            &plan,
            Vec::new(),
            &controls,
        )
        .map_err(CassieError::from)
    }
}
