use std::io;
use std::str;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::pgwire::protocol::{ReadyState, WireError};

const PROTOCOL_VERSION_3: i32 = 0x0003_0000;
const SSL_REQUEST_CODE: i32 = 80_877_103;
const CANCEL_REQUEST_CODE: i32 = 80_877_102;
const MIN_STARTUP_MESSAGE_BYTES: usize = 8;
const PASSWORD_MESSAGE_TAG: u8 = b'p';
const MAX_FRONTEND_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[path = "connection/blocking.rs"]
mod blocking;
#[path = "connection/codecs.rs"]
mod codecs;
#[path = "connection/copy.rs"]
mod copy;
#[path = "connection/errors.rs"]
mod errors;
#[path = "connection/extended.rs"]
mod extended;
#[path = "connection/readers.rs"]
mod readers;
#[path = "connection/simple_query.rs"]
mod simple_query;
#[path = "connection/startup_params.rs"]
mod startup_params;
#[path = "connection/state.rs"]
mod state;
#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;
#[path = "connection/transport.rs"]
mod transport;
#[path = "connection/writers.rs"]
mod writers;

use blocking::run_pgwire_blocking;
use errors::{cassie_pg_error, PgWireError, PgWireSeverity};
use readers::{
    read_frontend_message, read_password_message, read_simple_query_message, read_startup_frame,
};
use startup_params::validate_startup_parameters;
use state::{
    DescribeTarget, FrontendMessage, HandshakeError, HandshakeState, SessionState, StartupFrame,
};
use transport::PgwireTransport;

type PgwireReader = BufReader<tokio::io::ReadHalf<PgwireTransport>>;
use writers::{
    write_auth_cleartext, write_auth_ok, write_backend_key_data, write_copy_data, write_copy_done,
    write_copy_in_response, write_copy_out_response, write_error_response,
    write_parameter_statuses, write_ready_for_query, write_simple_query_result,
    write_ssl_not_supported,
};

pub(crate) fn benchmark_encode_data_row(
    row: Vec<crate::types::Value>,
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<Vec<u8>> {
    let mut frame = Vec::new();
    writers::append_data_row_frame(&mut frame, row, columns, result_formats)?;
    Ok(frame)
}

pub(crate) fn benchmark_decode_frontend(tag: u8, payload: Vec<u8>) -> Result<usize, String> {
    let payload_len = payload.len();
    let (message, consumed) =
        readers::decode_frontend_message(tag, payload).map_err(|error| format!("{error:?}"))?;
    if consumed != payload_len {
        return Err("frontend benchmark frame was not fully consumed".to_string());
    }
    std::hint::black_box(message);
    Ok(consumed)
}

pub(crate) fn benchmark_decode_parameter(
    parameter: &[u8],
    format: i16,
    oid: i32,
) -> Result<crate::types::Value, String> {
    extended::benchmark_decode_parameter(parameter, format, oid)
}

pub async fn run_connection(
    socket: TcpStream,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    require_tls: bool,
) {
    let runtime = cassie.runtime.clone();
    let _session_guard = runtime.begin_pgwire_session();
    let Ok(transport) = PgwireTransport::negotiate(socket, tls_config).await else {
        return;
    };
    if require_tls && !transport.is_tls() {
        return;
    }
    let (read_half, mut write_half) = tokio::io::split(transport);
    let mut reader = BufReader::new(read_half);
    let mut state = SessionState::new();
    let mut handshake_state = HandshakeState::AwaitStartup;
    let mut awaiting_sync = false;

    loop {
        let state_result = match handshake_state {
            HandshakeState::AwaitStartup => {
                handle_startup(
                    cassie.clone(),
                    &config,
                    &runtime,
                    &mut reader,
                    &mut write_half,
                    &mut state,
                )
                .await
            }
            HandshakeState::AwaitPassword {
                ref user,
                ref database,
            } => {
                handle_password(
                    cassie.clone(),
                    &runtime,
                    &mut reader,
                    &mut write_half,
                    &mut state,
                    user.clone(),
                    database.clone(),
                )
                .await
            }
            HandshakeState::Ready => {
                handle_ready(
                    cassie.clone(),
                    &runtime,
                    &mut reader,
                    &mut write_half,
                    &mut state,
                    &mut awaiting_sync,
                )
                .await
            }
        };

        match state_result {
            ConnectionStep::Continue(next_state) => handshake_state = next_state,
            ConnectionStep::Break => break,
        }
    }

    state.cleanup_pgwire_objects(&runtime);
}

#[cfg(debug_assertions)]
#[doc(hidden)]
/// Arms a one-shot retryable pgwire boundary failure for integration tests.
///
/// # Panics
///
/// Panics if the internal test hook registry is poisoned.
pub fn arm_next_pgwire_blocking_retryable_failure_for_test(
    cassie: &Cassie,
    message: impl Into<String>,
) {
    blocking::arm_next_retryable_failure_for_test(cassie, message);
}

pub(crate) async fn reject_too_many_connections(mut socket: TcpStream) {
    let error = PgWireError::too_many_connections();
    let _ = write_error_response(&mut socket, &error).await;
    let _ = socket.shutdown().await;
}

enum ConnectionStep {
    Continue(HandshakeState),
    Break,
}

async fn handle_startup(
    cassie: Arc<Cassie>,
    config: &CassieRuntimeConfig,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
) -> ConnectionStep {
    match read_startup_frame(reader).await {
        Ok(StartupFrame::SslRequest) => {
            let _ = write_ssl_not_supported(write_half).await;
            ConnectionStep::Continue(HandshakeState::AwaitStartup)
        }
        Ok(StartupFrame::Startup(parameters)) => {
            runtime.record_pgwire_message("startup");
            state.startup_user = Some(
                parameters
                    .get("user")
                    .cloned()
                    .unwrap_or_else(|| config.user.clone()),
            );
            state.startup_database = parameters.get("database").cloned();
            if let Err(error) = validate_startup_parameters(&parameters) {
                runtime.record_pgwire_protocol_error();
                let _ = write_error_response(write_half, &PgWireError::fatal_protocol(error)).await;
                return ConnectionStep::Break;
            }
            let startup_user = state
                .startup_user
                .clone()
                .unwrap_or_else(|| config.user.clone());
            let startup_database = state.startup_database.clone();
            if cassie.authentication_enabled() {
                let _ = write_auth_cleartext(write_half).await;
                ConnectionStep::Continue(HandshakeState::AwaitPassword {
                    user: startup_user,
                    database: startup_database,
                })
            } else {
                let session = cassie.create_session(&startup_user, startup_database.clone());
                if let Err(error) = cassie.ensure_session_database_exists(&session) {
                    runtime.record_pgwire_protocol_error();
                    let pg_error = PgWireError::from_cassie_error(PgWireSeverity::Fatal, &error);
                    let _ = write_error_response(write_half, &pg_error).await;
                    return ConnectionStep::Break;
                }
                state.authenticated = true;
                state.session = Some(session.clone());
                state.ready = ReadyState::Idle;
                runtime.record_pgwire_auth_ok();
                let registration = cassie.runtime.register_pgwire_backend();
                let _ = write_auth_ok(write_half).await;
                let _ = write_parameter_statuses(write_half).await;
                let _ = write_backend_key_data(
                    write_half,
                    registration.process_id(),
                    registration.secret_key(),
                )
                .await;
                state.backend_registration = Some(registration);
                let _ = write_ready_for_query(write_half, &session).await;
                ConnectionStep::Continue(HandshakeState::Ready)
            }
        }
        Ok(StartupFrame::CancelRequest {
            process_id,
            secret_key,
        }) => {
            runtime.cancel_pgwire_backend(process_id, secret_key);
            ConnectionStep::Break
        }
        Err(HandshakeError::Closed) => ConnectionStep::Break,
        Err(HandshakeError::Invalid(_)) => {
            runtime.record_pgwire_protocol_error();
            let _ = write_error_response(
                write_half,
                &PgWireError::fatal_protocol("invalid startup packet"),
            )
            .await;
            ConnectionStep::Break
        }
    }
}

async fn handle_password(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    user: String,
    database: Option<String>,
) -> ConnectionStep {
    match read_password_message(reader).await {
        Ok(password) => {
            runtime.record_pgwire_message("password");
            let auth_result = run_pgwire_blocking(cassie.clone(), "pgwire_auth", move |cassie| {
                cassie
                    .authenticate_principal(&user, Some(&password), database.clone())
                    .map(|principal| principal.session)
            })
            .await;
            match auth_result {
                Ok(session) => {
                    state.authenticated = true;
                    state.session = Some(session.clone());
                    state.ready = ReadyState::Idle;
                    runtime.record_pgwire_auth_ok();
                    let registration = cassie.runtime.register_pgwire_backend();
                    let _ = write_auth_ok(write_half).await;
                    let _ = write_parameter_statuses(write_half).await;
                    let _ = write_backend_key_data(
                        write_half,
                        registration.process_id(),
                        registration.secret_key(),
                    )
                    .await;
                    state.backend_registration = Some(registration);
                    let _ = write_ready_for_query(write_half, &session).await;
                    ConnectionStep::Continue(HandshakeState::Ready)
                }
                Err(CassieError::Unauthorized) => {
                    runtime.record_pgwire_auth_failed();
                    runtime.record_pgwire_protocol_error();
                    let _ = write_error_response(
                        write_half,
                        &PgWireError::auth_failed("authentication failed"),
                    )
                    .await;
                    ConnectionStep::Break
                }
                Err(error) => {
                    runtime.record_pgwire_protocol_error();
                    let pg_error = PgWireError::from_cassie_error(PgWireSeverity::Fatal, &error);
                    let _ = write_error_response(write_half, &pg_error).await;
                    ConnectionStep::Break
                }
            }
        }
        Err(HandshakeError::Closed) => ConnectionStep::Break,
        Err(HandshakeError::Invalid(error)) => {
            runtime.record_pgwire_protocol_error();
            let _ = write_error_response(
                write_half,
                &PgWireError::fatal_protocol(format!("invalid password message: {error}")),
            )
            .await;
            ConnectionStep::Break
        }
    }
}

async fn handle_ready(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    awaiting_sync: &mut bool,
) -> ConnectionStep {
    let next_tag = match reader.fill_buf().await {
        Ok(buffer) if !buffer.is_empty() => buffer[0],
        _ => return ConnectionStep::Break,
    };
    let Some(session) = state.session.clone() else {
        runtime.record_pgwire_protocol_error();
        let _ = write_error_response(
            write_half,
            &PgWireError::auth_required(WireError::NotAuthenticated.to_string()),
        )
        .await;
        return ConnectionStep::Continue(HandshakeState::Ready);
    };
    if *awaiting_sync {
        return handle_sync_wait(runtime, reader, write_half, awaiting_sync, &session).await;
    }
    if next_tag == b'Q' {
        return handle_simple_query(cassie, runtime, reader, write_half, state).await;
    }
    extended::handle_frontend_message(
        cassie,
        runtime,
        reader,
        write_half,
        state,
        awaiting_sync,
        &session,
    )
    .await
}

async fn handle_sync_wait(
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    awaiting_sync: &mut bool,
    session: &CassieSession,
) -> ConnectionStep {
    let message = match read_frontend_message(reader).await {
        Ok(message) => message,
        Err(HandshakeError::Closed) => return ConnectionStep::Break,
        Err(HandshakeError::Invalid(_)) => return ConnectionStep::Continue(HandshakeState::Ready),
    };
    match message {
        state::FrontendMessage::Sync => {
            runtime.record_pgwire_message("sync");
            *awaiting_sync = false;
            let _ = write_ready_for_query(write_half, session).await;
        }
        state::FrontendMessage::Terminate => return ConnectionStep::Break,
        state::FrontendMessage::Flush => runtime.record_pgwire_message("flush"),
        _ => {}
    }
    ConnectionStep::Continue(HandshakeState::Ready)
}

async fn handle_simple_query(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
) -> ConnectionStep {
    runtime.record_pgwire_message("query");
    runtime.record_pgwire_simple_query();
    let Some(session) = state.session.as_ref() else {
        runtime.record_pgwire_protocol_error();
        let _ = write_error_response(
            write_half,
            &PgWireError::auth_required(WireError::NotAuthenticated.to_string()),
        )
        .await;
        return ConnectionStep::Continue(HandshakeState::Ready);
    };
    let sql = match read_simple_query_message(reader).await {
        Ok(sql) => sql,
        Err(HandshakeError::Closed) => return ConnectionStep::Break,
        Err(HandshakeError::Invalid(error)) => {
            runtime.record_pgwire_protocol_error();
            let _ = write_error_response(
                write_half,
                &PgWireError::protocol(format!("invalid simple query message: {error}")),
            )
            .await;
            let _ = write_ready_for_query(write_half, session).await;
            return ConnectionStep::Continue(HandshakeState::Ready);
        }
    };

    let statements = match simple_query::split_simple_query(&sql) {
        Ok(statements) => statements,
        Err(simple_query::SplitError::Syntax(message)) => {
            runtime.record_pgwire_protocol_error();
            session.mark_transaction_failed();
            let error = PgWireError::new(PgWireSeverity::Error, "42601", message);
            if write_error_response(write_half, &error).await.is_err()
                || write_ready_for_query(write_half, session).await.is_err()
            {
                return ConnectionStep::Break;
            }
            return ConnectionStep::Continue(HandshakeState::Ready);
        }
        Err(simple_query::SplitError::Unsupported(message)) => {
            runtime.record_pgwire_protocol_error();
            session.mark_transaction_failed();
            let error = PgWireError::new(PgWireSeverity::Error, "0A000", message);
            if write_error_response(write_half, &error).await.is_err()
                || write_ready_for_query(write_half, session).await.is_err()
            {
                return ConnectionStep::Break;
            }
            return ConnectionStep::Continue(HandshakeState::Ready);
        }
    };

    if statements.len() == 1
        && matches!(
            copy::try_handle_simple_copy_query(
                cassie.clone(),
                session.clone(),
                &statements[0],
                reader,
                write_half
            )
            .await,
            copy::SimpleCopyOutcome::Handled
        )
    {
        return ConnectionStep::Continue(HandshakeState::Ready);
    }

    for statement in statements {
        match execute_simple_statement(
            cassie.clone(),
            runtime,
            write_half,
            state,
            session,
            statement,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => break,
            Err(()) => return ConnectionStep::Break,
        }
    }

    if write_ready_for_query(write_half, session).await.is_err() {
        return ConnectionStep::Break;
    }
    ConnectionStep::Continue(HandshakeState::Ready)
}

async fn execute_simple_statement(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &SessionState,
    session: &CassieSession,
    statement: String,
) -> Result<bool, ()> {
    let registration = state.backend_registration.as_ref().ok_or(())?;
    let cancellation = registration.begin_query();
    let cancellation_handle = cancellation.handle();
    let session = session.clone();
    let query_result = run_pgwire_blocking(cassie, "pgwire_simple_query", move |cassie| {
        cassie.execute_sql_with_cancellation(&session, &statement, Vec::new(), &cancellation_handle)
    })
    .await;
    drop(cancellation);

    match query_result {
        Ok(result) => write_simple_query_result(write_half, result)
            .await
            .map(|()| true)
            .map_err(|_| ()),
        Err(error) => {
            runtime.record_pgwire_protocol_error();
            let pg_error = cassie_pg_error(&error);
            write_error_response(write_half, &pg_error)
                .await
                .map(|()| false)
                .map_err(|_| ())
        }
    }
}
