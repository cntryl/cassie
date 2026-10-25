//! Ordered statement execution within an accepted Simple Query boundary.

use super::state::SessionState;
use super::{
    cassie_pg_error, execute_simple_statement, transactions, write_empty_query_response,
    write_error_response, AsyncWrite, Cassie, CassieSession,
};
use crate::runtime::RuntimeState;
use crate::sql::ast::{ParsedStatement, QueryStatement};
use std::sync::Arc;

pub(super) async fn run_batch(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    query: (Vec<String>, String),
    parsed: Vec<ParsedStatement>,
) -> Result<(), ()> {
    let (statements, frontend_sql) = query;
    let frontend_sql: Arc<str> = frontend_sql.into();
    if statements.is_empty() {
        write_empty_query_response(write_half)
            .await
            .map_err(|_| ())?;
    }
    for (statement, parsed) in statements.into_iter().zip(parsed) {
        let notice = missing_relation_notice(&cassie, &parsed);
        if !session.is_transaction_active()
            && !session.is_transaction_failed()
            && transactions::eligible(&parsed, &cassie.catalog)
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
            (statement, Arc::clone(&frontend_sql)),
            notice,
        )
        .await?
        {
            transactions::fail_query(runtime, state, session);
            break;
        }
    }
    Ok(())
}

fn missing_relation_notice(cassie: &Cassie, parsed: &ParsedStatement) -> Option<String> {
    match &parsed.statement {
        QueryStatement::DropTable(statement)
            if statement.if_exists && !cassie.catalog.exists(&statement.table) =>
        {
            Some(format!(
                "table \"{}\" does not exist, skipping",
                statement.table
            ))
        }
        QueryStatement::AlterTable(statement)
            if statement.if_exists && !cassie.catalog.exists(&statement.table) =>
        {
            Some(format!(
                "relation \"{}\" does not exist, skipping",
                statement.table
            ))
        }
        _ => None,
    }
}
