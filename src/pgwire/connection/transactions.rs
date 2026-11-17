//! Private operation classification for the selected wire transaction profile.

#[cfg(test)]
#[path = "transactions/portal_snapshot_test_hooks.rs"]
pub(super) mod portal_snapshot_test_hooks;

use super::state::SessionState;
use crate::app::{Cassie, CassieError, CassieSession, SessionTransactionStatus};
use crate::runtime::RuntimeState;
use crate::sql::ast::{ParsedStatement, QueryStatement, StatementFamily};
use std::sync::Arc;
use tokio::io::AsyncWrite;

/// Protocol-local failures capture contemporaneously on a blocking worker.
pub(super) async fn capture_protocol_owner(session: &CassieSession) {
    if !session.may_capture_protocol_owner() {
        return;
    }
    let session = session.clone();
    let _ = tokio::task::spawn_blocking(move || {
        crate::app::FailureAuthority::capture_for_operation(&session)
    })
    .await;
}

#[cfg(test)]
pub(super) fn observe_portal_cleanup(session: &CassieSession) {
    portal_snapshot_test_hooks::after_snapshot(session, session.transaction_status_snapshot());
}

pub(super) async fn capture_and_validate_protocol_owner(
    session: &CassieSession,
) -> Result<(), CassieError> {
    let session = session.clone();
    tokio::task::spawn_blocking(move || {
        let authority = crate::app::FailureAuthority::capture_for_operation(&session);
        session.ensure_admitted_operation_owner(&authority)
    })
    .await
    .map_err(|error| CassieError::Execution(format!("protocol owner worker failed: {error}")))?
}

pub(super) async fn fail_owned_transaction(session: &CassieSession) {
    let session = session.clone();
    let _ = tokio::task::spawn_blocking(move || session.fail_operation_transaction()).await;
}

pub(super) async fn fail_owned_query(
    runtime: &RuntimeState,
    state: &mut SessionState,
    session: &CassieSession,
) {
    fail_owned_transaction(session).await;
    clear_owned_idle_portals(runtime, state, session);
}

pub(super) fn clear_owned_idle_portals(
    runtime: &RuntimeState,
    state: &mut SessionState,
    session: &CassieSession,
) {
    if let Some(authority) = session.operation_failure_authority() {
        state.adopt_operation_portals(authority);
    }
    session.try_with_retired_operation_owner(|owner| state.clear_portals_owned_by(owner, runtime));
    session.try_with_current_portal_lifetime(|current| state.reconcile_portals(current, runtime));
}

#[cfg(test)]
pub(super) fn clear_idle_portals(
    runtime: &RuntimeState,
    state: &mut SessionState,
    session: &CassieSession,
) {
    if let Some(authority) = session.operation_failure_authority() {
        state.adopt_operation_portals(authority);
    }
    let status = session.transaction_status_snapshot();
    #[cfg(test)]
    portal_snapshot_test_hooks::after_snapshot(session, status);
    if status == SessionTransactionStatus::Idle {
        state.clear_all_portals(runtime);
    }
}

pub(super) async fn finish_query(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
) -> Result<(), std::io::Error> {
    if !session.operation_was_rejected() && session.is_implicit_transaction() {
        let transaction_session = session.clone();
        if let Err(error) =
            super::run_pgwire_blocking(cassie, "pgwire_implicit_commit", move |cassie| {
                cassie.commit_transaction(&transaction_session)
            })
            .await
        {
            fail_owned_transaction(session).await;
            super::write_error_response(write_half, &super::cassie_pg_error(&error)).await?;
        }
    }
    clear_owned_idle_portals(runtime, state, session);
    super::writers::write_changed_parameter_statuses(
        write_half,
        session,
        &mut state.parameter_statuses,
    )
    .await?;
    super::write_ready_for_query(write_half, session).await
}

#[derive(Debug, Default)]
pub(super) enum ExtendedCycle {
    #[default]
    Empty,
    Executing,
    StandaloneComplete,
}

impl ExtendedCycle {
    pub(super) fn start_execute(&mut self) -> Result<bool, CassieError> {
        match self {
            Self::Empty => {
                *self = Self::Executing;
                Ok(true)
            }
            Self::Executing => Ok(false),
            Self::StandaloneComplete => Err(CassieError::Unsupported(
                "standalone DDL requires Sync before another Execute".to_owned(),
            )),
        }
    }
}

pub(super) fn eligible(statement: &ParsedStatement, catalog: &crate::catalog::Catalog) -> bool {
    if statement.statement.family() == StatementFamily::Runtime {
        return !matches!(
            statement.statement,
            QueryStatement::Transaction(_) | QueryStatement::Copy(_)
        );
    }
    crate::app::implicit_ddl_supported(catalog, &statement.statement)
}

pub(super) fn preflight(
    cassie: &crate::app::Cassie,
    session: &crate::app::CassieSession,
    statements: &[String],
) -> Result<Vec<ParsedStatement>, CassieError> {
    let parsed = statements
        .iter()
        .map(|sql| cassie.parse_statement_for_session(Some(session), sql))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.len() > 1
        && parsed.iter().any(|statement| {
            statement.statement.family() != StatementFamily::Runtime
                || matches!(statement.statement, QueryStatement::Copy(_))
        })
    {
        return Err(CassieError::Unsupported(
            "DDL and catalog operations require a standalone Query".to_owned(),
        ));
    }
    Ok(parsed)
}
