use std::io;
use std::str;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::pgwire::handlers::query;
use crate::pgwire::protocol::{Portal, PortalSuspended, PreparedStatement, ReadyState, WireError};
use crate::types::Value;
use std::collections::HashMap;
use std::convert::TryFrom;

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
#[path = "connection/execute.rs"]
mod execute;
#[path = "connection/formats.rs"]
mod formats;
#[path = "connection/readers.rs"]
mod readers;
#[path = "connection/startup_params.rs"]
mod startup_params;
#[path = "connection/state.rs"]
mod state;
#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;
#[path = "connection/writers.rs"]
mod writers;

use blocking::run_pgwire_blocking;
use errors::{cassie_pg_error, PgWireError, PgWireSeverity};
use readers::{parse_bind_param_value, read_frontend_message, read_password_message, read_simple_query_message, read_startup_frame};
use startup_params::validate_startup_parameters;
use state::{CloseTarget, DescribeTarget, FrontendMessage, HandshakeError, HandshakeState, SessionState, StartupFrame};
use writers::{write_auth_cleartext, write_auth_ok, write_backend_frame, write_command_complete, write_copy_in_response, write_data_row, write_error_response, write_parameter_description, write_parameter_statuses, write_portal_suspended, write_ready_for_query, write_row_description, write_simple_query_result, write_ssl_not_supported, validate_result_formats};

pub async fn run_connection(
    mut socket: TcpStream,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
) {
    let runtime = cassie.runtime.clone();
    let _session_guard = runtime.begin_pgwire_session();
    let (read_half, mut write_half) = socket.split();
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
            HandshakeState::AwaitPassword { ref user, ref database } => {
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
            HandshakeState::Ready => handle_ready(
                cassie.clone(),
                &runtime,
                &mut reader,
                &mut write_half,
                &mut state,
                &mut awaiting_sync,
            )
            .await,
        };

        match state_result {
            ConnectionStep::Continue(next_state) => handshake_state = next_state,
            ConnectionStep::Break => break,
        }
    }
}

enum ConnectionStep {
    Continue(HandshakeState),
    Break,
}

async fn handle_startup(
    cassie: Arc<Cassie>,
    config: &CassieRuntimeConfig,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
) -> ConnectionStep {
    match read_startup_frame(reader).await {
        Ok(StartupFrame::SslRequest) => {
            let _ = write_ssl_not_supported(write_half).await;
            ConnectionStep::Continue(HandshakeState::AwaitStartup)
        }
        Ok(StartupFrame::CancelRequest) => ConnectionStep::Break,
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
            if config.password.is_empty() {
                state.authenticated = true;
                let session = cassie.create_session(&startup_user, startup_database.clone());
                state.session = Some(session.clone());
                state.ready = ReadyState::Idle;
                runtime.record_pgwire_auth_ok();
                let _ = write_auth_ok(write_half).await;
                let _ = write_parameter_statuses(write_half).await;
                let _ = write_ready_for_query(write_half, &session).await;
                ConnectionStep::Continue(HandshakeState::Ready)
            } else {
                let _ = write_auth_cleartext(write_half).await;
                ConnectionStep::Continue(HandshakeState::AwaitPassword {
                    user: startup_user,
                    database: startup_database,
                })
            }
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
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    user: String,
    database: Option<String>,
) -> ConnectionStep {
    match read_password_message(reader).await {
        Ok(password) => {
            runtime.record_pgwire_message("password");
            let auth_result = run_pgwire_blocking(cassie, "pgwire_auth", move |cassie| {
                cassie.authenticate_role(&user, Some(&password), database.clone())
            })
            .await;
            match auth_result {
                Ok(session) => {
                    state.authenticated = true;
                    state.session = Some(session.clone());
                    state.ready = ReadyState::Idle;
                    runtime.record_pgwire_auth_ok();
                    let _ = write_auth_ok(write_half).await;
                    let _ = write_parameter_statuses(write_half).await;
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
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
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
    if next_tag == b'Q' {
        return handle_simple_query(cassie, runtime, reader, write_half, state).await;
    }
    if *awaiting_sync {
        return handle_sync_wait(runtime, reader, write_half, awaiting_sync, &session).await;
    }
    ConnectionStep::Continue(HandshakeState::Ready)
}

async fn handle_sync_wait(
    runtime: &crate::runtime::RuntimeState,
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
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
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
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
    let session_for_query = session.clone();
    let sql_for_query = sql.clone();
    if matches!(
        copy::try_handle_simple_copy_query(cassie.clone(), session.clone(), &sql, reader, write_half).await,
        copy::SimpleCopyOutcome::Handled
    ) {
        return ConnectionStep::Continue(HandshakeState::Ready);
    }
    let query_result = run_pgwire_blocking(cassie, "pgwire_simple_query", move |cassie| {
        cassie.execute_sql(&session_for_query, &sql_for_query, Vec::new())
    })
    .await;
    if let Ok(result) = query_result {
        let _ = write_simple_query_result(write_half, result).await;
    } else if let Err(error) = query_result {
        runtime.record_pgwire_protocol_error();
        let pg_error = cassie_pg_error(&error);
        let _ = write_error_response(write_half, &pg_error).await;
    }
    let _ = write_ready_for_query(write_half, session).await;
    ConnectionStep::Continue(HandshakeState::Ready)
}
