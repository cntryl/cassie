//! `DISCARD {ALL | PLANS | SEQUENCES | TEMP}`.
//!
//! `DISCARD ALL` closes cursors, resets every setting to its connection
//! default, deallocates SQL and protocol prepared statements and forgets the
//! session's sequence history. Cassie compiles plans per statement through a
//! settings-keyed cache, so `DISCARD PLANS` has nothing session-owned to drop,
//! and it has no session-scoped temporary relations for `DISCARD TEMP`.

use super::command_result;
use crate::app::{Cassie, CassieError, CassieSession, QueryExecutionControls, QueryResult};
use crate::sql::ast::DiscardTarget;

impl Cassie {
    pub(super) fn execute_discard(
        session: &CassieSession,
        target: DiscardTarget,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let command = crate::sql::ast::SessionObjectStatement::Discard(target).command_name();
        if target == DiscardTarget::All {
            session.validate_discard_all_owner()?;
        }
        let output = command_result(command, controls)?;
        match target {
            DiscardTarget::All => {
                session.discard_all_owned_state()?;
            }
            DiscardTarget::Sequences => session.discard_sequence_values_for_operation()?,
            DiscardTarget::Plans | DiscardTarget::Temp => {}
        }
        Ok(output)
    }
}
