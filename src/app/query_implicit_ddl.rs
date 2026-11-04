//! Publish an implicit standalone table before exposing its successful command.

use std::time::Duration;

use super::{
    Cassie, CassieError, CassieSession, ExecutionMode, QueryExecutionControls, QueryResult, Value,
};
use crate::sql::ParsedStatement;

impl Cassie {
    pub(super) fn execute_implicit_table_creation(
        &self,
        session: &CassieSession,
        parsed: ParsedStatement,
        fingerprint: u64,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        // Both ordinary wire protocols already require a standalone DDL
        // statement. Keep its complete publication inside this worker, before
        // either protocol can expose CommandComplete or release this guard.
        let publication = lock_publication(self, controls, || {})?;
        let result = self.execute_bound_catalog_scoped(
            session,
            parsed,
            fingerprint,
            parameters,
            mode,
            controls,
        )?;
        // The scoped execution has retired its thread-local staging scope.
        // Committing within that scope would stage the publication itself.
        self.commit_transaction_holding_catalog(session, publication, controls)?;
        Ok(result)
    }
}

fn lock_publication<'a>(
    cassie: &'a Cassie,
    controls: &QueryExecutionControls,
    mut before_wait: impl FnMut(),
) -> Result<parking_lot::RwLockWriteGuard<'a, ()>, CassieError> {
    loop {
        controls.check_interruption()?;
        before_wait();
        if let Some(publication) = cassie
            .catalog_publication
            .try_write_for(Duration::from_millis(2))
        {
            controls.check_interruption()?;
            return Ok(publication);
        }
    }
}

#[cfg(test)]
mod tests;
