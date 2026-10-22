use super::state::SessionState;
use super::writers::write_command_complete;
use super::{
    cassie_pg_error, read_frontend_message, run_pgwire_blocking, write_copy_data, write_copy_done,
    write_copy_in_response, write_copy_out_response, write_error_response, write_ready_for_query,
    Arc, AsyncWrite, Cassie, CassieError, CassieSession, FrontendMessage, HandshakeError,
    PgWireError, PgWireSeverity, PgwireReader, MAX_FRONTEND_MESSAGE_BYTES,
};
use crate::sql::ast::CopyStatement;

#[path = "copy/input.rs"]
mod input;
#[path = "copy/output.rs"]
mod output;
#[path = "copy/request.rs"]
mod request;

use request::{CopyDirection, CopyRequest, CopyWireFormat};

pub(super) enum SimpleCopyOutcome {
    Handled,
    NotCopy,
    ConnectionClosed,
}

pub(super) async fn handle_simple_copy_candidate(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    state: &mut SessionState,
    session: &CassieSession,
    sql: &str,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let streaming = super::simple_query::is_streaming_copy(sql);
    if streaming && cassie.ensure_session_database_access(session).is_err() {
        super::transactions::fail_query(runtime, state, session);
        return write_copy_error(
            write_half,
            session,
            &cassie_pg_error(&CassieError::InsufficientPrivilege),
        )
        .await;
    }
    let cancellation = state
        .backend_registration
        .as_ref()
        .filter(|_| streaming)
        .map(crate::runtime::PgwireBackendRegistration::begin_query);
    let handle = cancellation
        .as_ref()
        .map(crate::runtime::PgwireQueryCancellationGuard::handle);
    let outcome = try_handle_simple_copy_query(
        cassie,
        session.clone(),
        sql,
        handle.as_ref(),
        reader,
        write_half,
    )
    .await;
    drop(cancellation);
    if matches!(outcome, SimpleCopyOutcome::Handled) {
        super::transactions::clear_idle_portals(runtime, state, session);
    }
    outcome
}

fn binding_context(cassie: &Cassie, session: &CassieSession) -> crate::sql::binder::BindingContext {
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database.as_str())
        .to_string();
    let search_path = cassie.binding_search_path(session);
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
    let bound = match bind_copy_request(cassie.clone(), session.clone(), sql).await {
        Ok(Some(bound)) => bound,
        Ok(None) => return SimpleCopyOutcome::NotCopy,
        Err(error) => {
            return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
        }
    };

    if session.is_implicit_transaction() {
        let error = cassie_pg_error(&CassieError::Unsupported(
            "table COPY cannot join pending implicit SQL work".to_owned(),
        ));
        return write_copy_error(write_half, &session, &error).await;
    }
    if session.is_transaction_failed() {
        let error = cassie_pg_error(&CassieError::failed_transaction());
        return write_copy_error(write_half, &session, &error).await;
    }

    match bound {
        BoundCopyRequest::From(request, statement) => {
            let column_count = copy_response_column_count(&cassie, &statement);
            if write_copy_in_response(
                write_half,
                column_count,
                output::response_format(request.format),
            )
            .await
            .is_err()
            {
                return SimpleCopyOutcome::ConnectionClosed;
            }
            if handle_simple_copy_from_stdin(
                cassie,
                session,
                request,
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
        BoundCopyRequest::To(request, query_sql) => {
            handle_simple_copy_to_stdout(
                cassie,
                session,
                request,
                query_sql,
                cancellation.cloned(),
                write_half,
            )
            .await
        }
    }
}

async fn bind_copy_request(
    cassie: Arc<Cassie>,
    session: CassieSession,
    sql: &str,
) -> Result<Option<BoundCopyRequest>, CassieError> {
    let sql = sql.to_string();
    let context = binding_context(&cassie, &session);
    let session_for_parse = session.clone();
    run_pgwire_blocking(cassie, "pgwire_copy_parse", move |cassie| {
        let Some(request) = request::parse_copy_request(&sql)? else {
            return Ok(None);
        };
        match request.direction {
            CopyDirection::FromStdin => {
                let binding_sql = request.table_binding_sql().ok_or_else(|| {
                    CassieError::Parse("COPY FROM STDIN requires a table target".to_string())
                })?;
                let parsed = crate::sql::parser::parse_statement(&binding_sql)?;
                session_for_parse.authorize_statement(&parsed.statement)?;
                Cassie::ensure_transaction_recovery(&session_for_parse, &parsed)?;
                session_for_parse.ensure_transaction_writable()?;
                session_for_parse.mark_transaction_query();
                let bound =
                    crate::sql::binder::bind_with_context(parsed, &cassie.catalog, &context)?;
                let crate::sql::ast::QueryStatement::Copy(statement) = bound.statement.statement
                else {
                    return Err(CassieError::Parse(
                        "COPY FROM STDIN did not bind as a table copy".to_string(),
                    ));
                };
                Ok(Some(BoundCopyRequest::From(request, statement)))
            }
            CopyDirection::ToStdout => {
                let query_sql = request.query_sql();
                let parsed = crate::sql::parser::parse_statement(&query_sql)?;
                if !matches!(
                    &parsed.statement,
                    crate::sql::ast::QueryStatement::Select(_)
                ) {
                    return Err(CassieError::Unsupported(
                        "COPY TO STDOUT requires a SELECT query".to_string(),
                    ));
                }
                session_for_parse.authorize_statement(&parsed.statement)?;
                Cassie::ensure_transaction_recovery(&session_for_parse, &parsed)?;
                session_for_parse.mark_transaction_query();
                Ok(Some(BoundCopyRequest::To(request, query_sql)))
            }
        }
    })
    .await
}

enum BoundCopyRequest {
    From(CopyRequest, CopyStatement),
    To(CopyRequest, String),
}

async fn handle_simple_copy_to_stdout(
    cassie: Arc<Cassie>,
    session: CassieSession,
    request: CopyRequest,
    query_sql: String,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    let session_for_query = session.clone();
    let result = run_pgwire_blocking(cassie.clone(), "pgwire_copy_to_stdout", move |cassie| {
        if let Some(cancellation) = cancellation.as_ref() {
            cassie.execute_sql_with_cancellation(
                &session_for_query,
                &query_sql,
                Vec::new(),
                cancellation,
            )
        } else {
            cassie.execute_sql(&session_for_query, &query_sql, Vec::new())
        }
    })
    .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
        }
    };

    let columns = result.columns();
    let header = output::encode_header(&request, columns);
    if write_copy_out_response(
        write_half,
        columns.len(),
        output::response_format(request.format),
    )
    .await
    .is_err()
    {
        return SimpleCopyOutcome::ConnectionClosed;
    }

    if request.format == CopyWireFormat::Binary
        && write_copy_data(write_half, &output::binary_header())
            .await
            .is_err()
    {
        return SimpleCopyOutcome::ConnectionClosed;
    }
    if let Some(header) = header {
        if write_copy_data(write_half, &header).await.is_err() {
            return SimpleCopyOutcome::ConnectionClosed;
        }
    }
    for row in result.rows() {
        let encoded = match output::encode_row(&request, columns, row) {
            Ok(encoded) => encoded,
            Err(error) => {
                return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await
            }
        };
        if write_copy_data(write_half, &encoded).await.is_err() {
            return SimpleCopyOutcome::ConnectionClosed;
        }
    }
    if request.format == CopyWireFormat::Binary
        && write_copy_data(write_half, &output::binary_trailer())
            .await
            .is_err()
    {
        return SimpleCopyOutcome::ConnectionClosed;
    }
    let command = format!("COPY {}", result.row_count());
    if write_copy_done(write_half).await.is_err()
        || write_command_complete(write_half, &command).await.is_err()
        || write_ready_for_query(write_half, &session).await.is_err()
    {
        SimpleCopyOutcome::ConnectionClosed
    } else {
        SimpleCopyOutcome::Handled
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
    if session.is_implicit_transaction() {
        session.rollback_transaction();
    } else {
        session.mark_transaction_failed();
    }
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
    if write_copy_out_response(write_half, 1, 0).await.is_err() {
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
    if write_copy_in_response(write_half, 1, 0).await.is_err() {
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
    request: CopyRequest,
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
                    request,
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
    request: CopyRequest,
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
            &request,
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
    request: &CopyRequest,
    statement: &CopyStatement,
    payload: &[u8],
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
) -> Result<String, CassieError> {
    let mut rows = match request.format {
        CopyWireFormat::Csv => crate::app::parse_copy_csv_payload(
            payload,
            request.csv_delimiter(),
            request.null_marker(),
        )?,
        CopyWireFormat::Text => {
            input::parse_text_payload(payload, request.text_delimiter(), request.null_marker())?
        }
        CopyWireFormat::Binary => input::parse_binary_payload(payload, cassie, statement)?,
    };
    if request.header && !rows.is_empty() {
        rows.remove(0);
    }
    let count = cassie.ingest_copy_rows(session, statement, rows, cancellation)?;
    Ok(format!("COPY {count}"))
}
