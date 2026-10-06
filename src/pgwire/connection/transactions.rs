//! Private operation classification for the selected wire transaction profile.

use super::state::SessionState;
use crate::app::{Cassie, CassieError, CassieSession};
use crate::runtime::RuntimeState;
use crate::sql::ast::{ParsedStatement, QueryStatement, StatementFamily};
use std::sync::Arc;
use tokio::io::AsyncWrite;

pub(super) fn fail_query(
    runtime: &RuntimeState,
    state: &mut SessionState,
    session: &CassieSession,
) {
    if session.is_implicit_transaction() {
        session.rollback_transaction();
    } else {
        session.mark_transaction_failed();
    }
    clear_idle_portals(runtime, state, session);
}

pub(super) fn clear_idle_portals(
    runtime: &RuntimeState,
    state: &mut SessionState,
    session: &CassieSession,
) {
    if !session.is_transaction_active() && !session.is_transaction_failed() {
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
    if session.is_implicit_transaction() {
        let transaction_session = session.clone();
        if let Err(error) =
            super::run_pgwire_blocking(cassie, "pgwire_implicit_commit", move |cassie| {
                cassie.commit_transaction(&transaction_session)
            })
            .await
        {
            session.rollback_transaction();
            super::write_error_response(write_half, &super::cassie_pg_error(&error)).await?;
        }
    }
    clear_idle_portals(runtime, state, session);
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

pub(super) fn eligible(statement: &ParsedStatement) -> bool {
    statement.statement.family() == StatementFamily::Runtime
        && !matches!(
            statement.statement,
            QueryStatement::Transaction(_) | QueryStatement::Copy(_)
        )
}

pub(super) fn preflight(statements: &[String]) -> Result<Vec<ParsedStatement>, CassieError> {
    let parsed = statements
        .iter()
        .map(|sql| crate::sql::parser::parse_statement(sql).map_err(CassieError::from))
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
