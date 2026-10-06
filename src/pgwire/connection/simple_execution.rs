//! Ordered statement execution within an accepted Simple Query boundary.

use super::state::SessionState;
use super::{
    cassie_pg_error, execute_simple_statement, transactions, write_empty_query_response,
    write_error_response, AsyncWrite, Cassie, CassieSession,
};
use crate::runtime::RuntimeState;
use crate::sql::ast::ParsedStatement;
use std::sync::Arc;

pub(super) async fn run_batch(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    statements: Vec<String>,
    parsed: Vec<ParsedStatement>,
) -> Result<(), ()> {
    if statements.is_empty() {
        write_empty_query_response(write_half)
            .await
            .map_err(|_| ())?;
    }
    for (statement, parsed) in statements.into_iter().zip(parsed) {
        if !session.is_transaction_active()
            && !session.is_transaction_failed()
            && transactions::eligible(&parsed)
        {
            if let Err(error) = session.begin_implicit_transaction() {
                let _ = write_error_response(write_half, &cassie_pg_error(&error)).await;
                break;
            }
        }
        if !execute_simple_statement(
            cassie.clone(),
            runtime,
            write_half,
            state,
            session,
            statement,
        )
        .await?
        {
            transactions::fail_query(runtime, state, session);
            break;
        }
    }
    Ok(())
}
