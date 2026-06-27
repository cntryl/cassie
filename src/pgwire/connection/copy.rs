use super::*;
use crate::sql::ast::{CopyStatement, QueryStatement};

pub(super) enum SimpleCopyOutcome {
    Handled,
    NotCopy,
    ConnectionClosed,
}

pub(super) async fn try_handle_simple_copy_query(
    cassie: Arc<Cassie>,
    session: CassieSession,
    sql: &str,
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let sql = sql.to_string();
    let statement = match run_pgwire_blocking(cassie.clone(), "pgwire_copy_parse", move |cassie| {
        if !sql.trim_start().to_ascii_lowercase().starts_with("copy ") {
            return Ok(None);
        }
        let parsed = crate::sql::parser::parse_statement(&sql)?;
        let QueryStatement::Copy(_) = &parsed.statement else {
            return Ok(None);
        };
        let bound = crate::sql::binder::bind(parsed, &cassie.catalog)?;
        let QueryStatement::Copy(statement) = bound.statement.statement else {
            return Ok(None);
        };
        Ok(Some(statement))
    })
    .await
    {
        Ok(Some(statement)) => statement,
        Ok(None) => return SimpleCopyOutcome::NotCopy,
        Err(error) => {
            let pg_error = cassie_pg_error(&error);
            if write_error_response(write_half, &pg_error).await.is_err()
                || write_ready_for_query(write_half, &session).await.is_err()
            {
                return SimpleCopyOutcome::ConnectionClosed;
            }
            return SimpleCopyOutcome::Handled;
        }
    };

    let column_count = copy_response_column_count(&cassie, &statement);
    if write_copy_in_response(write_half, column_count)
        .await
        .is_err()
    {
        return SimpleCopyOutcome::ConnectionClosed;
    }
    if handle_simple_copy_from_stdin(cassie, session, statement, reader, write_half).await {
        SimpleCopyOutcome::Handled
    } else {
        SimpleCopyOutcome::ConnectionClosed
    }
}

fn copy_response_column_count(cassie: &Cassie, statement: &CopyStatement) -> usize {
    if !statement.columns.is_empty() {
        return statement.columns.len();
    }
    cassie
        .catalog
        .get_schema(&statement.table)
        .map(|schema| schema.fields.len())
        .unwrap_or(0)
}

async fn handle_simple_copy_from_stdin(
    cassie: Arc<Cassie>,
    session: CassieSession,
    statement: CopyStatement,
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> bool {
    let mut payload = Vec::new();

    loop {
        match read_frontend_message(reader).await {
            Ok(FrontendMessage::CopyData(chunk)) => {
                let Some(next_len) = payload.len().checked_add(chunk.len()) else {
                    let error = PgWireError::protocol("COPY payload exceeds supported bounds");
                    if write_error_response(write_half, &error).await.is_err() {
                        return false;
                    }
                    return write_ready_for_query(write_half, &session).await.is_ok();
                };
                if next_len > MAX_FRONTEND_MESSAGE_BYTES {
                    let error = PgWireError::protocol("COPY payload exceeds supported bounds");
                    if write_error_response(write_half, &error).await.is_err() {
                        return false;
                    }
                    return write_ready_for_query(write_half, &session).await.is_ok();
                }
                payload.extend_from_slice(&chunk);
            }
            Ok(FrontendMessage::CopyDone) => {
                let session_for_copy = session.clone();
                let statement_for_copy = statement.clone();
                let result =
                    run_pgwire_blocking(cassie.clone(), "pgwire_copy_from_stdin", move |cassie| {
                        cassie
                            .copy_from_csv_stdin(&session_for_copy, &statement_for_copy, &payload)
                            .map(|count| format!("COPY {count}"))
                    })
                    .await;

                match result {
                    Ok(command) => {
                        if write_command_complete(write_half, &command).await.is_err() {
                            return false;
                        }
                    }
                    Err(error) => {
                        let pg_error = cassie_pg_error(&error);
                        if write_error_response(write_half, &pg_error).await.is_err() {
                            return false;
                        }
                    }
                }
                return write_ready_for_query(write_half, &session).await.is_ok();
            }
            Ok(FrontendMessage::CopyFail(message)) => {
                let error = PgWireError::new(
                    PgWireSeverity::Error,
                    "57014",
                    format!("COPY failed: {message}"),
                );
                if write_error_response(write_half, &error).await.is_err() {
                    return false;
                }
                return write_ready_for_query(write_half, &session).await.is_ok();
            }
            Ok(FrontendMessage::Terminate) | Err(HandshakeError::Closed) => return false,
            Ok(_) => {
                let error = PgWireError::protocol(
                    "unexpected frontend message during COPY FROM STDIN".to_string(),
                );
                if write_error_response(write_half, &error).await.is_err() {
                    return false;
                }
                return write_ready_for_query(write_half, &session).await.is_ok();
            }
            Err(HandshakeError::Invalid(error)) => {
                let error = PgWireError::protocol(format!("invalid COPY message: {error}"));
                if write_error_response(write_half, &error).await.is_err() {
                    return false;
                }
                return write_ready_for_query(write_half, &session).await.is_ok();
            }
        }
    }
}
