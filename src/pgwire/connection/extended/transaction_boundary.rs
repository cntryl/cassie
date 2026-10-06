//! Operation admission before portal execution and its fast paths.

use super::{Cassie, CassieSession, ExtendedQueryError, RuntimeState, SessionState};
use crate::sql::ast::{ParsedStatement, StatementFamily};

pub(super) fn prepare_execute(
    state: &mut SessionState,
    session: &CassieSession,
    portal_name: &str,
    max_rows: i32,
) -> Result<(usize, bool), ExtendedQueryError> {
    let max_rows = usize::try_from(max_rows)
        .map_err(|_| ExtendedQueryError::protocol("invalid execute row limit"))?;
    let first = state
        .extended_cycle
        .start_execute()
        .map_err(|error| ExtendedQueryError::cassie(&error))?;
    if let Some(parsed) = state
        .portals
        .get(portal_name)
        .and_then(|portal| state.prepared_statements.get(&portal.statement_name))
        .and_then(|prepared| prepared.parsed.as_ref())
    {
        Cassie::ensure_transaction_recovery(session, parsed)
            .map_err(|error| ExtendedQueryError::cassie(&error))?;
    }
    Ok((max_rows, first))
}

pub(super) fn admit_statement(
    state: &mut SessionState,
    session: &CassieSession,
    parsed: &ParsedStatement,
    first_execute: bool,
) -> Result<bool, ExtendedQueryError> {
    let standalone = parsed.statement.family() != StatementFamily::Runtime;
    if standalone {
        if !first_execute {
            return Err(ExtendedQueryError::unsupported(
                "standalone DDL must be the first Execute after Sync",
            ));
        }
        state.extended_cycle = super::super::transactions::ExtendedCycle::StandaloneComplete;
    }
    if !session.is_transaction_active()
        && !session.is_transaction_failed()
        && super::super::transactions::eligible(parsed)
    {
        session
            .begin_implicit_transaction()
            .map_err(|error| ExtendedQueryError::cassie(&error))?;
    }
    Ok(standalone)
}

pub(super) fn close_completed_owner(
    state: &mut SessionState,
    runtime: &RuntimeState,
    session: &CassieSession,
    was_in_transaction: bool,
) {
    if was_in_transaction && !session.is_transaction_active() && !session.is_transaction_failed() {
        state.clear_all_portals(runtime);
    }
}
