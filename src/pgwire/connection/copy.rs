use super::writers::write_command_complete;
use super::{
    cassie_pg_error, read_frontend_message, run_pgwire_blocking, str, write_copy_data,
    write_copy_done, write_copy_in_response, write_copy_out_response, write_error_response,
    write_ready_for_query, Arc, AsyncWrite, Cassie, CassieError, CassieSession, FrontendMessage,
    HandshakeError, PgWireError, PgWireSeverity, PgwireReader, MAX_FRONTEND_MESSAGE_BYTES,
};
use crate::sql::ast::{CopyStatement, QueryStatement};

pub(super) enum SimpleCopyOutcome {
    Handled,
    NotCopy,
    ConnectionClosed,
}

fn binding_context(cassie: &Cassie, session: &CassieSession) -> crate::sql::binder::BindingContext {
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database.as_str())
        .to_string();
    let search_path = session.search_path();
    if cassie.database_catalog_enforced() {
        crate::sql::binder::BindingContext::scoped(database, search_path)
    } else {
        crate::sql::binder::BindingContext::unscoped(database, search_path)
    }
}

pub(super) async fn try_handle_simple_copy_query(
    cassie: Arc<Cassie>,
    session: CassieSession,
    sql: &str,
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    if let Some(command) = parse_database_copy_command(sql) {
        return handle_database_copy(cassie, session, command, reader, write_half).await;
    }
    let sql = sql.to_string();
    let context = binding_context(&cassie, &session);
    let session_for_parse = session.clone();
    let statement = match run_pgwire_blocking(cassie.clone(), "pgwire_copy_parse", move |cassie| {
        if !sql.trim_start().to_ascii_lowercase().starts_with("copy ") {
            return Ok(None);
        }
        let parsed = crate::sql::parser::parse_statement(&sql)?;
        session_for_parse.authorize_statement(&parsed.statement)?;
        let QueryStatement::Copy(_) = &parsed.statement else {
            return Ok(None);
        };
        let bound = crate::sql::binder::bind_with_context(parsed, &cassie.catalog, &context)?;
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
            return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
        }
    };

    if session.is_transaction_failed() {
        let error = cassie_pg_error(&CassieError::Execution(
            "transaction is failed; rollback required".to_string(),
        ));
        return write_copy_error(write_half, &session, &error).await;
    }

    let column_count = copy_response_column_count(&cassie, &statement);
    if write_copy_in_response(write_half, column_count)
        .await
        .is_err()
    {
        return SimpleCopyOutcome::ConnectionClosed;
    }
    if handle_simple_copy_from_stdin(
        cassie,
        session,
        statement,
        cancellation.cloned(),
        reader,
        write_half,
    )
    .await
    {
        SimpleCopyOutcome::Handled
    } else {
        SimpleCopyOutcome::ConnectionClosed
    }
}

#[derive(Debug, Clone)]
enum DatabaseCopyCommand {
    Backup { source: String },
    Restore { target: String },
}

fn parse_database_copy_command(sql: &str) -> Option<DatabaseCopyCommand> {
    let tokens = sql
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>();
    if tokens.len() == 5
        && tokens[0].eq_ignore_ascii_case("backup")
        && tokens[1].eq_ignore_ascii_case("database")
        && tokens[3].eq_ignore_ascii_case("to")
        && tokens[4].eq_ignore_ascii_case("stdout")
    {
        return Some(DatabaseCopyCommand::Backup {
            source: tokens[2].to_string(),
        });
    }
    if tokens.len() == 5
        && tokens[0].eq_ignore_ascii_case("restore")
        && tokens[1].eq_ignore_ascii_case("database")
        && tokens[3].eq_ignore_ascii_case("from")
        && tokens[4].eq_ignore_ascii_case("stdin")
    {
        return Some(DatabaseCopyCommand::Restore {
            target: tokens[2].to_string(),
        });
    }
    None
}

async fn handle_database_copy(
    cassie: Arc<Cassie>,
    session: CassieSession,
    command: DatabaseCopyCommand,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let database = match &command {
        DatabaseCopyCommand::Backup { source } => source,
        DatabaseCopyCommand::Restore { target } => target,
    };
    let can_access_database = cassie
        .catalog
        .get_role(&session.user)
        .is_some_and(|role| role.can_access_database(database));
    if !can_access_database || session.is_authenticated_read_only() {
        let error = cassie_pg_error(&CassieError::InsufficientPrivilege);
        return write_copy_error(write_half, &session, &error).await;
    }
    if session.is_transaction_failed() || session.is_transaction_active() {
        let error = cassie_pg_error(&CassieError::Unsupported(
            "database backup and restore are not supported inside an explicit transaction"
                .to_string(),
        ));
        return write_copy_error(write_half, &session, &error).await;
    }

    match command {
        DatabaseCopyCommand::Backup { source } => {
            handle_database_backup(cassie, session, source, write_half).await
        }
        DatabaseCopyCommand::Restore { target } => {
            handle_database_restore(cassie, session, target, reader, write_half).await
        }
    }
}

/// Ends a COPY-family statement with `ErrorResponse` + `ReadyForQuery`.
///
/// Any statement error aborts an open explicit transaction, so the status byte
/// reports `E` and `COMMIT` cannot apply work issued after the failure. Copy
/// messages the client is still streaming are dropped by the main message
/// loop, as the protocol requires once the backend has left copy-in mode.
async fn write_copy_error(
    write_half: &mut (impl AsyncWrite + Unpin),
    session: &CassieSession,
    error: &PgWireError,
) -> SimpleCopyOutcome {
    session.mark_transaction_failed();
    if write_error_response(write_half, error).await.is_err()
        || write_ready_for_query(write_half, session).await.is_err()
    {
        SimpleCopyOutcome::ConnectionClosed
    } else {
        SimpleCopyOutcome::Handled
    }
}

async fn handle_database_backup(
    cassie: Arc<Cassie>,
    session: CassieSession,
    source: String,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let stream = run_pgwire_blocking(
        cassie.clone(),
        "pgwire_database_backup_begin",
        move |cassie| cassie.begin_database_backup(&source),
    )
    .await;
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(error) => {
            return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
        }
    };
    // The backup image is streamed as a single text-format column.
    if write_copy_out_response(write_half, 1).await.is_err() {
        return SimpleCopyOutcome::ConnectionClosed;
    }
    loop {
        let result =
            run_pgwire_blocking(cassie.clone(), "pgwire_database_backup_chunk", move |_| {
                let chunk = stream.next_chunk()?;
                Ok((stream, chunk))
            })
            .await;
        match result {
            Ok((next_stream, Some(chunk))) => {
                stream = next_stream;
                if write_copy_data(write_half, &chunk).await.is_err() {
                    return SimpleCopyOutcome::ConnectionClosed;
                }
            }
            Ok((_, None)) => {
                if write_copy_done(write_half).await.is_err()
                    || write_command_complete(write_half, "BACKUP").await.is_err()
                    || write_ready_for_query(write_half, &session).await.is_err()
                {
                    return SimpleCopyOutcome::ConnectionClosed;
                }
                return SimpleCopyOutcome::Handled;
            }
            Err(error) => {
                return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await;
            }
        }
    }
}

async fn handle_database_restore(
    cassie: Arc<Cassie>,
    session: CassieSession,
    target: String,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let restore = run_pgwire_blocking(
        cassie.clone(),
        "pgwire_database_restore_begin",
        move |cassie| cassie.begin_database_restore(&target),
    )
    .await;
    let restore = match restore {
        Ok(restore) => restore,
        Err(error) => {
            return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
        }
    };
    if write_copy_in_response(write_half, 1).await.is_err() {
        return SimpleCopyOutcome::ConnectionClosed;
    }

    consume_database_restore(cassie, session, restore, reader, write_half).await
}

async fn consume_database_restore(
    cassie: Arc<Cassie>,
    session: CassieSession,
    mut restore: crate::app::DatabaseRestoreSession,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    loop {
        let error = match read_frontend_message(reader).await {
            Ok(FrontendMessage::CopyData(chunk)) => {
                match push_database_restore_chunk(cassie.clone(), restore, chunk).await {
                    Ok(Ok(next_restore)) => {
                        restore = next_restore;
                        continue;
                    }
                    Ok(Err((next_restore, error))) => {
                        let _ = abort_database_restore(cassie.clone(), next_restore).await;
                        cassie_pg_error(&error)
                    }
                    Err(error) => cassie_pg_error(&error),
                }
            }
            // The protocol ignores Flush and Sync while in copy-in mode.
            Ok(FrontendMessage::Flush | FrontendMessage::Sync) => continue,
            Ok(FrontendMessage::CopyDone) => {
                return finish_database_restore(cassie, session, restore, write_half).await;
            }
            Ok(FrontendMessage::CopyFail(message)) => {
                let _ = abort_database_restore(cassie, restore).await;
                PgWireError::new(
                    PgWireSeverity::Error,
                    "57014",
                    format!("RESTORE failed: {message}"),
                )
            }
            Ok(FrontendMessage::Terminate) | Err(HandshakeError::Closed) => {
                let _ = abort_database_restore(cassie, restore).await;
                return SimpleCopyOutcome::ConnectionClosed;
            }
            Ok(_) | Err(HandshakeError::Invalid(_)) => {
                let _ = abort_database_restore(cassie, restore).await;
                PgWireError::protocol("unexpected frontend message during RESTORE FROM STDIN")
            }
        };
        return write_copy_error(write_half, &session, &error).await;
    }
}

async fn push_database_restore_chunk(
    cassie: Arc<Cassie>,
    restore: crate::app::DatabaseRestoreSession,
    chunk: Vec<u8>,
) -> Result<
    Result<crate::app::DatabaseRestoreSession, (crate::app::DatabaseRestoreSession, CassieError)>,
    CassieError,
> {
    let result = run_pgwire_blocking(cassie, "pgwire_database_restore_chunk", move |_| {
        let mut restore = restore;
        let pushed = restore.push_chunk(&chunk);
        Ok((restore, pushed))
    })
    .await;
    match result {
        Ok((restore, Ok(()))) => Ok(Ok(restore)),
        Ok((restore, Err(error))) => Ok(Err((restore, error))),
        Err(error) => Err(error),
    }
}

async fn finish_database_restore(
    cassie: Arc<Cassie>,
    session: CassieSession,
    restore: crate::app::DatabaseRestoreSession,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let result = run_pgwire_blocking(
        cassie.clone(),
        "pgwire_database_restore_finish",
        move |_| {
            let mut restore = restore;
            let finished = restore.finish();
            Ok((restore, finished))
        },
    )
    .await;
    let error = match result {
        Ok((_, Ok(()))) => {
            if write_command_complete(write_half, "RESTORE").await.is_err()
                || write_ready_for_query(write_half, &session).await.is_err()
            {
                return SimpleCopyOutcome::ConnectionClosed;
            }
            return SimpleCopyOutcome::Handled;
        }
        Ok((restore, Err(error))) => {
            let _ = abort_database_restore(cassie, restore).await;
            error
        }
        Err(error) => error,
    };
    write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
}

async fn abort_database_restore(
    cassie: Arc<Cassie>,
    mut restore: crate::app::DatabaseRestoreSession,
) -> Result<(), crate::app::CassieError> {
    run_pgwire_blocking(cassie, "pgwire_database_restore_abort", move |_| {
        restore.abort()
    })
    .await
}

fn copy_response_column_count(cassie: &Cassie, statement: &CopyStatement) -> usize {
    if !statement.columns.is_empty() {
        return statement.columns.len();
    }
    cassie
        .catalog
        .get_schema(&statement.table)
        .map_or(0, |schema| schema.fields.len())
}

/// Buffers one `COPY ... FROM STDIN` stream and ingests it at `CopyDone`.
///
/// The whole stream is applied as one atomic batch, so the buffered payload
/// is bounded by `MAX_FRONTEND_MESSAGE_BYTES`; exceeding it is a resource
/// limit (`54000`), not a protocol violation. Returns `false` when the
/// connection must close.
async fn handle_simple_copy_from_stdin(
    cassie: Arc<Cassie>,
    session: CassieSession,
    statement: CopyStatement,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> bool {
    let mut payload = Vec::new();

    let error = loop {
        match read_frontend_message(reader).await {
            Ok(FrontendMessage::CopyData(chunk)) => {
                if payload.len().saturating_add(chunk.len()) > MAX_FRONTEND_MESSAGE_BYTES {
                    break cassie_pg_error(&CassieError::ResourceLimit(format!(
                        "COPY FROM STDIN payload exceeds {MAX_FRONTEND_MESSAGE_BYTES} bytes"
                    )));
                }
                payload.extend_from_slice(&chunk);
            }
            // The protocol ignores Flush and Sync while in copy-in mode.
            Ok(FrontendMessage::Flush | FrontendMessage::Sync) => {}
            Ok(FrontendMessage::CopyDone) => {
                return finish_simple_copy(
                    cassie,
                    session,
                    statement,
                    payload,
                    cancellation,
                    write_half,
                )
                .await;
            }
            Ok(FrontendMessage::CopyFail(message)) => {
                break PgWireError::new(
                    PgWireSeverity::Error,
                    "57014",
                    format!("COPY failed: {message}"),
                );
            }
            Ok(FrontendMessage::Terminate) | Err(HandshakeError::Closed) => return false,
            Ok(_) => {
                break PgWireError::protocol("unexpected frontend message during COPY FROM STDIN");
            }
            Err(HandshakeError::Invalid(error)) => {
                break PgWireError::protocol(format!("invalid COPY message: {error}"));
            }
        }
    };
    drop(payload);
    matches!(
        write_copy_error(write_half, &session, &error).await,
        SimpleCopyOutcome::Handled
    )
}

async fn finish_simple_copy(
    cassie: Arc<Cassie>,
    session: CassieSession,
    statement: CopyStatement,
    payload: Vec<u8>,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> bool {
    let session_for_copy = session.clone();
    let result = run_pgwire_blocking(cassie, "pgwire_copy_from_stdin", move |cassie| {
        execute_copy_payload(
            &cassie,
            &session_for_copy,
            &statement,
            &payload,
            cancellation.as_ref(),
        )
    })
    .await;
    match result {
        Ok(command) => {
            write_command_complete(write_half, &command).await.is_ok()
                && write_ready_for_query(write_half, &session).await.is_ok()
        }
        Err(error) => matches!(
            write_copy_error(write_half, &session, &cassie_pg_error(&error)).await,
            SimpleCopyOutcome::Handled
        ),
    }
}

fn execute_copy_payload(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &CopyStatement,
    payload: &[u8],
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
) -> Result<String, CassieError> {
    let count = cassie.ingest_copy_csv_payload(session, statement, payload, cancellation)?;
    Ok(format!("COPY {count}"))
}
