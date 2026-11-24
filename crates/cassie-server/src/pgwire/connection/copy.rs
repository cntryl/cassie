use super::state::SessionState;
use super::writers::write_command_complete;
use super::{
    cassie_pg_error, read_frontend_message, run_pgwire_blocking, write_copy_data, write_copy_done,
    write_copy_in_response, write_copy_out_response, write_error_response, write_notice_response,
    write_ready_for_query, Arc, AsyncWrite, Cassie, CassieError, CassieSession, FrontendMessage,
    HandshakeError, PgWireError, PgWireSeverity, PgwireReader, MAX_FRONTEND_MESSAGE_BYTES,
};
use crate::sql::ast::CopyStatement;
use std::collections::HashMap;

#[path = "copy/diagnostics.rs"]
mod diagnostics;
use diagnostics::{write_copy_jsonb_diagnostic_with_length, write_copy_unknown_column_with_length};

#[path = "copy/binding.rs"]
mod binding;
#[path = "copy/execute.rs"]
mod execute;
#[path = "copy/filter.rs"]
mod filter;
#[cfg(test)]
#[path = "copy/filter_text.rs"]
mod filter_text;
#[path = "copy/input.rs"]
mod input;
#[path = "copy/input_read.rs"]
mod input_read;
#[path = "copy/input_text.rs"]
mod input_text;
#[path = "copy/output.rs"]
mod output;
#[path = "copy/payload_owner.rs"]
mod payload_owner;
#[path = "copy/request.rs"]
mod request;
#[cfg(test)]
#[path = "copy/validation.rs"]
mod validation;

#[cfg(test)]
#[path = "copy/binding_accounting_test_hooks.rs"]
mod binding_accounting_test_hooks;
#[cfg(test)]
#[path = "copy/binding_authority_accounting_tests.rs"]
mod binding_authority_accounting_tests;
#[cfg(test)]
#[path = "copy/origin_failure_authority_tests.rs"]
mod origin_failure_authority_tests;
#[cfg(test)]
#[path = "copy/outer_failure_tests.rs"]
mod outer_failure_tests;

#[cfg(test)]
#[path = "copy/cancellation_tests.rs"]
pub(super) mod cancellation_tests;

#[cfg(test)]
#[path = "copy/payload_limit_tests.rs"]
mod payload_limit_tests;

#[cfg(test)]
#[path = "copy/diagnostic_tests.rs"]
mod diagnostic_tests;

#[cfg(test)]
#[path = "copy/stale_entry_tests.rs"]
mod stale_entry_tests;

#[cfg(test)]
#[path = "copy/input_text_tests.rs"]
mod input_text_tests;

#[cfg(test)]
#[path = "copy/output_tests.rs"]
mod output_tests;

use binding::CopyFromBinding;
use input_read::{read_admitted_copy_message, CopyReadOutcome};
#[cfg(test)]
use input_read::{read_copy_frontend_message, select_copy_read};

use payload_owner::{copy_payload_exceeds_limit, CopyPayload};
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
    let operation_session = streaming.then(|| session.with_new_wire_operation());
    let session = operation_session.as_ref().unwrap_or(session);
    if streaming {
        super::transactions::capture_protocol_owner(session).await;
    }
    if streaming && cassie.ensure_session_database_access(session).is_err() {
        super::transactions::fail_owned_query(runtime, state, session).await;
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
        &mut state.parameter_statuses,
        handle.as_ref(),
        reader,
        write_half,
    )
    .await;
    drop(cancellation);
    if matches!(outcome, SimpleCopyOutcome::Handled) {
        super::transactions::clear_owned_idle_portals(runtime, state, session);
    }
    outcome
}

fn binding_context(cassie: &Cassie, session: &CassieSession) -> crate::sql::binder::BindingContext {
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database())
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
    parameter_statuses: &mut HashMap<String, String>,
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> SimpleCopyOutcome {
    if super::simple_query::is_streaming_copy(sql) {
        if let Err(error) = cassie.check_storage_recovery_required() {
            return write_copy_error(write_half, &session, &cassie_pg_error(&error)).await;
        }
    }
    if let Some(command) = parse_database_copy_command(sql) {
        return handle_database_copy(cassie, session, command, reader, write_half).await;
    }
    let bound = match bind_copy_request(cassie.clone(), session.clone(), sql, cancellation.cloned())
        .await
    {
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
        BoundCopyRequest::From(request, _statement, binding) => {
            let column_count = binding.operation.copy_response_column_count();
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
                Arc::clone(&cassie),
                *binding,
                session,
                request,
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
                parameter_statuses,
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
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
) -> Result<Option<BoundCopyRequest>, CassieError> {
    let sql = sql.to_string();
    let session_for_parse = session.clone();
    run_pgwire_blocking(cassie, "pgwire_copy_parse", move |cassie| {
        let Some(request) = request::parse_copy_request(&sql)? else {
            return Ok(None);
        };
        cassie.check_storage_recovery_required()?;
        match request.direction {
            CopyDirection::FromStdin => {
                bind_copy_from_stdin(&cassie, &session_for_parse, request, cancellation).map(Some)
            }
            CopyDirection::ToStdout => {
                bind_copy_to_stdout(&cassie, &session_for_parse, request).map(Some)
            }
        }
    })
    .await
}

fn bind_copy_from_stdin(
    cassie: &Cassie,
    session: &CassieSession,
    request: CopyRequest,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
) -> Result<BoundCopyRequest, CassieError> {
    let binding_sql = request
        .table_binding_sql()
        .ok_or_else(|| CassieError::Parse("COPY FROM STDIN requires a table target".to_string()))?;
    let force_null = engine_force_selection(&request.force_null);
    let force_not_null = engine_force_selection(&request.force_not_null);
    let operation = crate::app::transport::copy::CopyFromOperation::bind(
        Arc::new(cassie.clone()),
        session,
        &binding_sql,
        &force_null,
        &force_not_null,
        cancellation,
    )?;
    let statement = operation.statement().clone();
    #[cfg(test)]
    let test_context = (
        Arc::clone(operation.cassie_for_test()),
        operation.owner_for_test(),
        operation.controls().clone(),
        operation.authority_for_test(),
    );
    Ok(BoundCopyRequest::From(
        request,
        Box::new(statement),
        Box::new(CopyFromBinding {
            operation,
            #[cfg(test)]
            cassie: test_context.0,
            #[cfg(test)]
            owner: test_context.1,
            #[cfg(test)]
            controls: test_context.2,
            #[cfg(test)]
            authority: test_context.3,
        }),
    ))
}

fn engine_force_selection(
    selection: &request::ColumnSelection,
) -> crate::app::transport::copy::CopyForceSelection {
    match selection {
        request::ColumnSelection::None => crate::app::transport::copy::CopyForceSelection::None,
        request::ColumnSelection::All => crate::app::transport::copy::CopyForceSelection::All,
        request::ColumnSelection::Columns(names) => {
            crate::app::transport::copy::CopyForceSelection::Columns(names.clone())
        }
    }
}

fn bind_copy_to_stdout(
    cassie: &Cassie,
    session: &CassieSession,
    request: CopyRequest,
) -> Result<BoundCopyRequest, CassieError> {
    let context = binding_context(cassie, session);
    let table_columns = match &request.target {
        request::CopyTarget::Table {
            relation, columns, ..
        } if columns.is_empty() => {
            let resolved =
                crate::sql::binder::resolve_relation_name(relation, &cassie.catalog, &context)?;
            cassie.catalog.get_schema(&resolved).map(|schema| {
                schema
                    .fields
                    .iter()
                    .map(|field| crate::catalog::object_identity::quote_identifier(&field.name))
                    .collect::<Vec<_>>()
            })
        }
        _ => None,
    };
    let query_sql = request.query_sql(table_columns.as_deref());
    let parsed = cassie.parse_statement_for_session(Some(session), &query_sql)?;
    if !matches!(
        &parsed.statement,
        crate::sql::ast::QueryStatement::Select(_)
    ) {
        return Err(CassieError::Unsupported(
            "COPY TO STDOUT requires a SELECT query".to_string(),
        ));
    }
    session.authorize_statement(&parsed.statement)?;
    cassie.authorize_role_object_access(session, &parsed.statement)?;
    Cassie::ensure_transaction_recovery(session, &parsed)?;
    session.mark_transaction_query();
    Ok(BoundCopyRequest::To(request, query_sql))
}

enum BoundCopyRequest {
    From(CopyRequest, Box<CopyStatement>, Box<CopyFromBinding>),
    To(CopyRequest, String),
}

async fn handle_simple_copy_to_stdout(
    cassie: Arc<Cassie>,
    session: CassieSession,
    request: CopyRequest,
    query_sql: String,
    parameter_statuses: &mut HashMap<String, String>,
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
        || super::writers::write_changed_parameter_statuses(
            write_half,
            &session,
            parameter_statuses,
        )
        .await
        .is_err()
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
    fail_copy_transaction(session).await;
    if write_error_response(write_half, error).await.is_err()
        || write_ready_for_query(write_half, session).await.is_err()
    {
        SimpleCopyOutcome::ConnectionClosed
    } else {
        SimpleCopyOutcome::Handled
    }
}

async fn fail_copy_transaction(session: &CassieSession) {
    super::transactions::fail_owned_transaction(session).await;
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

/// Buffers one `COPY ... FROM STDIN` stream and ingests it at `CopyDone`.
///
/// The whole stream is applied as one atomic batch, so the buffered payload
/// is bounded by `MAX_FRONTEND_MESSAGE_BYTES`; exceeding it is a resource
/// limit (`54000`), not a protocol violation. Returns `false` when the
/// connection must close.
async fn handle_simple_copy_from_stdin(
    cassie: Arc<Cassie>,
    binding: CopyFromBinding,
    session: CassieSession,
    request: CopyRequest,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> bool {
    let mut payload = match CopyPayload::new(&binding.controls()) {
        Ok(payload) => payload,
        Err(error) => {
            return matches!(
                write_copy_error(write_half, &session, &cassie_pg_error(&error)).await,
                SimpleCopyOutcome::Handled
            )
        }
    };

    let error = loop {
        let (message, _frame_memory) = match read_admitted_copy_message(
            cancellation.as_ref(),
            binding.controls().deadline,
            &binding.controls(),
            reader,
        )
        .await
        {
            CopyReadOutcome::Canceled => break cassie_pg_error(&CassieError::QueryCanceled),
            CopyReadOutcome::DeadlineIdle => break cassie_pg_error(&CassieError::DeadlineExceeded),
            CopyReadOutcome::DeadlinePartial | CopyReadOutcome::FramingInvalid => return false,
            CopyReadOutcome::Resource(error) => break cassie_pg_error(&error),
            CopyReadOutcome::AdmittedMessage(Ok((message, memory))) => (Ok(message), Some(memory)),
            CopyReadOutcome::AdmittedMessage(Err(error)) => (Err(error), None),
            #[cfg(test)]
            CopyReadOutcome::Message(_) => {
                unreachable!("admitted reader always retains frame memory")
            }
        };
        match message {
            Ok(FrontendMessage::CopyData(chunk)) => {
                if cancellation
                    .as_ref()
                    .is_some_and(crate::runtime::QueryCancellationHandle::is_canceled)
                {
                    break cassie_pg_error(&CassieError::QueryCanceled);
                }
                if copy_payload_exceeds_limit(
                    payload.len(),
                    chunk.len(),
                    MAX_FRONTEND_MESSAGE_BYTES,
                ) {
                    break cassie_pg_error(&CassieError::ResourceLimit(format!(
                        "COPY FROM STDIN payload exceeds {MAX_FRONTEND_MESSAGE_BYTES} bytes"
                    )));
                }
                if let Err(error) = payload.append(&chunk, &binding.controls()) {
                    break cassie_pg_error(&error);
                }
                #[cfg(test)]
                cancellation_tests::signal_copy_payload_buffered();
            }
            // The protocol ignores Flush and Sync while in copy-in mode.
            Ok(FrontendMessage::Flush | FrontendMessage::Sync) => {}
            Ok(FrontendMessage::CopyDone) => {
                return finish_simple_copy(
                    cassie,
                    binding,
                    session,
                    request,
                    payload,
                    cancellation,
                    write_half,
                )
                .await;
            }
            Ok(FrontendMessage::CopyFail(message)) => {
                let (message, _error_memory) =
                    match payload_owner::copy_fail_message(&message, &binding.controls()) {
                        Ok(value) => value,
                        Err(error) => {
                            return matches!(
                                write_copy_error(write_half, &session, &cassie_pg_error(&error))
                                    .await,
                                SimpleCopyOutcome::Handled
                            )
                        }
                    };
                let error = PgWireError::new(PgWireSeverity::Error, "57014", message);
                return matches!(
                    write_copy_error(write_half, &session, &error).await,
                    SimpleCopyOutcome::Handled
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
    binding: CopyFromBinding,
    session: CassieSession,
    request: CopyRequest,
    payload: CopyPayload,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
    write_half: &mut (impl AsyncWrite + Unpin),
) -> bool {
    let session_for_copy = session.clone();
    let controls_for_error = binding.controls().clone();
    let result = super::blocking::run_pgwire_blocking_with_error(
        Arc::clone(&cassie),
        "pgwire_copy_from_stdin",
        move |cassie| {
            binding.operation.with_publication_scope(|controls| {
                execute::execute_decoded_operation(
                    &session_for_copy,
                    &request,
                    &binding.operation,
                    payload.as_slice(),
                    cancellation.as_ref(),
                    controls,
                )
            })
        },
    )
    .await;
    #[cfg(test)]
    super::wire_failure_test_hooks::after_worker(&cassie, &session);
    match result {
        Ok(completion) => {
            for notice in &completion.notices {
                if write_notice_response(write_half, notice).await.is_err() {
                    return false;
                }
            }
            write_command_complete(write_half, &completion.command)
                .await
                .is_ok()
                && write_ready_for_query(write_half, &session).await.is_ok()
        }
        Err(crate::app::transport::copy::CopyOperationError::Cassie(error)) => matches!(
            write_copy_error(write_half, &session, &cassie_pg_error(&error)).await,
            SimpleCopyOutcome::Handled
        ),
        Err(crate::app::transport::copy::CopyOperationError::Undefined(name)) => {
            write_copy_unknown_column_with_length(
                write_half,
                &session,
                &name,
                name.len(),
                &controls_for_error,
            )
            .await
        }
        Err(crate::app::transport::copy::CopyOperationError::Jsonb(owner)) => {
            let handled = write_copy_jsonb_diagnostic_with_length(
                write_half,
                &session,
                owner.as_str(),
                owner.as_str().len(),
                &controls_for_error,
            )
            .await;
            drop(owner);
            handled
        }
    }
}

#[cfg(all(test, feature = "replay-benchmark-observation"))]
#[path = "copy/native_policy_tests.rs"]
mod native_policy_tests;
