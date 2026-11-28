use std::io;
use std::net::IpAddr;
use std::str;
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
mod wire_failure_boundary_tests;
#[cfg(test)]
mod wire_failure_test_hooks;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
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
const MAX_STARTUP_MESSAGE_BYTES: usize = 64 * 1024;
const PGWIRE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
#[path = "connection/idle_portal_snapshot_tests.rs"]
mod idle_portal_snapshot_tests;

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
mod portal_reads;
#[path = "connection/readers.rs"]
mod readers;
#[path = "connection/result_frames.rs"]
mod result_frames;
#[path = "connection/simple_execution.rs"]
mod simple_execution;
#[path = "connection/simple_query.rs"]
mod simple_query;
#[path = "connection/startup_params.rs"]
mod startup_params;
#[path = "connection/state.rs"]
mod state;
#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;
#[path = "connection/transactions.rs"]
mod transactions;
#[path = "connection/transport.rs"]
mod transport;
pub(crate) mod worker_lease;
#[path = "connection/writers.rs"]
mod writers;

use blocking::run_pgwire_blocking;
use errors::{cassie_pg_error, PgWireError, PgWireSeverity};
#[cfg(test)]
use readers::read_frontend_message_after_tag;
use readers::{
    read_frontend_message, read_frontend_tag, read_password_message, read_simple_query_message,
    read_startup_frame,
};
use startup_params::{apply_startup_parameters, validate_startup_parameters};
use state::{
    DescribeTarget, FrontendMessage, HandshakeError, HandshakeState, SessionState, StartupFrame,
};
use transport::PgwireTransport;

type PgwireReader =
    BufReader<tokio::io::ReadHalf<crate::transport::TimedWriteTransport<PgwireTransport>>>;
use writers::{
    write_auth_cleartext, write_auth_ok, write_backend_key_data, write_copy_data, write_copy_done,
    write_copy_in_response, write_copy_out_response, write_empty_query_response,
    write_error_response, write_notice_response, write_parameter_statuses, write_ready_for_query,
    write_simple_query_result, write_ssl_not_supported,
};

pub(crate) fn benchmark_encode_data_row(
    row: &[crate::types::Value],
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<Vec<u8>> {
    let mut frame = Vec::new();
    writers::append_data_row_frame(&mut frame, row, columns, result_formats, &[])?;
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
    peer_ip: IpAddr,
) {
    let Ok(iam) = super::iam::configure(&cassie, &config, tls_config.is_some()) else {
        return;
    };
    Box::pin(run_connection_with_iam(
        socket,
        cassie,
        config,
        tls_config,
        require_tls,
        peer_ip,
        iam,
    ))
    .await;
}

pub(super) async fn run_connection_with_iam(
    socket: TcpStream,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    require_tls: bool,
    peer_ip: IpAddr,
    iam: Option<Arc<super::iam::Verifier>>,
) {
    let runtime = cassie.runtime().clone();
    let _session_guard = runtime.begin_pgwire_session();
    let Ok(Ok(transport)) = tokio::time::timeout(
        PGWIRE_HANDSHAKE_TIMEOUT,
        PgwireTransport::negotiate(socket, tls_config),
    )
    .await
    else {
        return;
    };
    let tls = transport.is_tls();
    if require_tls && !tls {
        return;
    }
    let transport = crate::transport::TimedWriteTransport::new(
        transport,
        Duration::from_millis(config.limits.pgwire_write_timeout_ms),
    );
    let (read_half, mut write_half) = tokio::io::split(transport);
    let mut reader = BufReader::new(read_half);
    let mut state = SessionState::new(&runtime.limits());
    let mut handshake_state = HandshakeState::AwaitStartup;
    let mut awaiting_sync = false;

    loop {
        let state_result = match handshake_state {
            HandshakeState::AwaitStartup => {
                match tokio::time::timeout(
                    PGWIRE_HANDSHAKE_TIMEOUT,
                    handle_startup(
                        cassie.clone(),
                        &runtime,
                        &mut reader,
                        &mut write_half,
                        &mut state,
                        iam.as_ref().map(|_| tls),
                    ),
                )
                .await
                {
                    Ok(step) => step,
                    Err(_) => ConnectionStep::Break,
                }
            }
            HandshakeState::AwaitPassword {
                ref user,
                ref database,
            } => {
                match tokio::time::timeout(
                    PGWIRE_HANDSHAKE_TIMEOUT,
                    handle_password(
                        cassie.clone(),
                        &runtime,
                        &mut reader,
                        &mut write_half,
                        &mut state,
                        PasswordAuthContext {
                            user: user.clone(),
                            database: database.clone(),
                            peer_ip,
                            iam: iam
                                .as_deref()
                                .filter(|_| cassie.is_workload_pgwire_selector(user)),
                        },
                    ),
                )
                .await
                {
                    Ok(step) => step,
                    Err(_) => ConnectionStep::Break,
                }
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
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    iam_tls: Option<bool>,
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
                    .unwrap_or_else(|| "root".to_string()),
            );
            state.startup_database = parameters.get("database").cloned();
            if let Err(error) = validate_startup_parameters(&parameters) {
                runtime.record_pgwire_protocol_error();
                let _ = write_error_response(write_half, &PgWireError::fatal_protocol(error)).await;
                return ConnectionStep::Break;
            }
            state.startup_parameters = parameters;
            let startup_user = state
                .startup_user
                .clone()
                .unwrap_or_else(|| "root".to_string());
            let startup_database = state.startup_database.clone();
            if cassie.is_workload_pgwire_selector(&startup_user) && iam_tls == Some(false) {
                runtime.record_pgwire_auth_failed();
                let _ = write_error_response(
                    write_half,
                    &PgWireError::auth_failed("authentication failed"),
                )
                .await;
                return ConnectionStep::Break;
            }
            if cassie.authentication_enabled()
                || (cassie.is_workload_pgwire_selector(&startup_user) && iam_tls.is_some())
            {
                let _ = write_auth_cleartext(write_half).await;
                ConnectionStep::Continue(HandshakeState::AwaitPassword {
                    user: startup_user,
                    database: startup_database,
                })
            } else {
                let session = cassie.create_session(&startup_user, startup_database.clone());
                if let Err(error) = apply_startup_parameters(&session, &state.startup_parameters) {
                    let _ = write_error_response(
                        write_half,
                        &PgWireError::from_cassie_error(PgWireSeverity::Fatal, &error),
                    )
                    .await;
                    return ConnectionStep::Break;
                }
                if let Err(error) = cassie.ensure_session_database_exists(&session) {
                    runtime.record_pgwire_protocol_error();
                    let pg_error = PgWireError::from_cassie_error(PgWireSeverity::Fatal, &error);
                    let _ = write_error_response(write_half, &pg_error).await;
                    return ConnectionStep::Break;
                }
                finish_startup(&cassie, runtime, write_half, state, session).await
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

async fn finish_startup(
    cassie: &Cassie,
    runtime: &crate::runtime::RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: CassieSession,
) -> ConnectionStep {
    state.authenticated = true;
    state.session = Some(session.clone());
    state.ready = ReadyState::Idle;
    runtime.record_pgwire_auth_ok();
    let registration = cassie.runtime().register_pgwire_backend();
    session.set_backend_pid(registration.process_id());
    let _ = write_auth_ok(write_half).await;
    state.parameter_statuses = match write_parameter_statuses(write_half, &session).await {
        Ok(statuses) => statuses,
        Err(_) => return ConnectionStep::Break,
    };
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

struct PasswordAuthContext<'a> {
    user: String,
    database: Option<String>,
    peer_ip: IpAddr,
    iam: Option<&'a super::iam::Verifier>,
}

async fn authenticate_startup_credential(
    cassie: Arc<Cassie>,
    auth: PasswordAuthContext<'_>,
    credential: String,
) -> Result<CassieSession, CassieError> {
    let PasswordAuthContext {
        user,
        database,
        peer_ip,
        iam,
    } = auth;
    if let Some(verifier) = iam {
        let attempt = cassie.begin_aws_iam_login(peer_ip)?;
        let identity = verifier
            .verify(&credential)
            .await
            .map_err(|_| CassieError::Unauthorized)?;
        if user
            .trim()
            .eq_ignore_ascii_case(crate::config::AWS_IAM_PGWIRE_SELECTOR)
            && !identity.is_aws
        {
            return Err(CassieError::Unauthorized);
        }
        drop(credential);
        return run_pgwire_blocking(cassie, "pgwire_auth", move |cassie| {
            cassie.complete_aws_iam_login(attempt, &identity.issuer, &identity.role_arn, database)
        })
        .await;
    }
    run_pgwire_blocking(cassie, "pgwire_auth", move |cassie| {
        cassie
            .authenticate_network_principal(&user, Some(&credential), database, peer_ip)
            .map(|principal| principal.session)
    })
    .await
}

async fn handle_password(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    auth: PasswordAuthContext<'_>,
) -> ConnectionStep {
    let credential_limit = auth.iam.map(|_| super::iam::MAX_TOKEN_BYTES);
    match read_password_message(reader, credential_limit).await {
        Ok(credential) => {
            runtime.record_pgwire_message("password");
            let auth_result =
                authenticate_startup_credential(cassie.clone(), auth, credential).await;
            match auth_result {
                Ok(session) => {
                    if let Err(error) =
                        apply_startup_parameters(&session, &state.startup_parameters)
                    {
                        let pg_error =
                            PgWireError::from_cassie_error(PgWireSeverity::Fatal, &error);
                        let _ = write_error_response(write_half, &pg_error).await;
                        return ConnectionStep::Break;
                    }
                    finish_startup(&cassie, runtime, write_half, state, session).await
                }
                Err(CassieError::Unauthorized | CassieError::AuthenticationRateLimited) => {
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
        return handle_sync_wait(
            cassie,
            runtime,
            reader,
            write_half,
            state,
            awaiting_sync,
            &session,
        )
        .await;
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
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
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
            state.extended_cycle = transactions::ExtendedCycle::default();
            #[cfg(test)]
            if !state.invalidate_portals_on_sync {
                transactions::observe_portal_cleanup(session);
            }
            if std::mem::take(&mut state.invalidate_portals_on_sync) {
                if let Some(owner) = state.portal_invalidation_owner.take() {
                    if let Some(identity) = owner.identity() {
                        state.clear_portals_owned_by(identity, runtime);
                    }
                }
            }
            let operation = session.with_new_wire_operation();
            transactions::capture_protocol_owner(&operation).await;
            if transactions::finish_query(cassie, runtime, write_half, state, &operation)
                .await
                .is_err()
            {
                return ConnectionStep::Break;
            }
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
    let Some(session) = state.session.clone() else {
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
            return write_simple_query_error(
                runtime,
                write_half,
                state,
                &session,
                PgWireError::protocol(format!("invalid simple query message: {error}")),
            )
            .await;
        }
    };

    if let Some(step) =
        reject_quarantined_simple_query(&cassie, runtime, write_half, state, &session, &sql).await
    {
        return step;
    }
    state.reset_unnamed_query_objects(runtime);
    state.extended_cycle = transactions::ExtendedCycle::default();
    let statements = match simple_query::split_simple_query(&sql) {
        Ok(statements) => statements,
        Err(error) => {
            return write_simple_query_split_error(runtime, write_half, state, &session, error)
                .await;
        }
    };

    if statements.len() == 1 {
        match copy::handle_simple_copy_candidate(
            cassie.clone(),
            runtime,
            state,
            &session,
            &statements[0],
            reader,
            write_half,
        )
        .await
        {
            copy::SimpleCopyOutcome::Handled => {
                return ConnectionStep::Continue(HandshakeState::Ready)
            }
            copy::SimpleCopyOutcome::ConnectionClosed => return ConnectionStep::Break,
            copy::SimpleCopyOutcome::NotCopy => {}
        }
    }

    let parsed = match transactions::preflight(&cassie, &session, &statements) {
        Ok(parsed) => parsed,
        Err(error) => {
            return write_simple_query_error(
                runtime,
                write_half,
                state,
                &session,
                cassie_pg_error(&error),
            )
            .await;
        }
    };

    let Ok(operation) = simple_execution::run_batch(
        cassie.clone(),
        runtime,
        write_half,
        state,
        &session,
        (statements, sql),
        parsed,
    )
    .await
    else {
        return ConnectionStep::Break;
    };

    if transactions::finish_query(cassie, runtime, write_half, state, &operation)
        .await
        .is_err()
    {
        return ConnectionStep::Break;
    }
    ConnectionStep::Continue(HandshakeState::Ready)
}

async fn write_simple_query_split_error(
    runtime: &crate::runtime::RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    error: simple_query::SplitError,
) -> ConnectionStep {
    let (sqlstate, message, protocol_error) = match error {
        simple_query::SplitError::Syntax(message) => ("42601", message, true),
        simple_query::SplitError::DuplicateColumn(message) => ("42701", message, false),
        simple_query::SplitError::NumericRange(message) => ("22003", message, true),
        simple_query::SplitError::UndefinedFunction(message) => ("42883", message, true),
        simple_query::SplitError::UndefinedObject(message) => ("42704", message, true),
        simple_query::SplitError::Unsupported(message) => ("0A000", message, true),
        simple_query::SplitError::ResourceLimit(message) => ("54000", message, false),
    };
    if protocol_error {
        runtime.record_pgwire_protocol_error();
    }
    let error = PgWireError::new(PgWireSeverity::Error, sqlstate, message);
    write_simple_query_error(runtime, write_half, state, session, error).await
}

async fn write_simple_query_error(
    runtime: &crate::runtime::RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    error: PgWireError,
) -> ConnectionStep {
    let operation = session.with_new_wire_operation();
    transactions::capture_protocol_owner(&operation).await;
    transactions::fail_owned_query(runtime, state, &operation).await;
    if writers::write_changed_parameter_statuses(write_half, session, &mut state.parameter_statuses)
        .await
        .is_err()
        || write_error_response(write_half, &error).await.is_err()
        || write_ready_for_query(write_half, session).await.is_err()
    {
        ConnectionStep::Break
    } else {
        ConnectionStep::Continue(HandshakeState::Ready)
    }
}

async fn execute_simple_statement(
    cassie: Arc<Cassie>,
    runtime: &crate::runtime::RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    query: (String, Arc<str>),
    notice: Option<String>,
) -> Result<bool, ()> {
    let registration = state.backend_registration.as_ref().ok_or(())?;
    let cancellation = registration.begin_query();
    let cancellation_handle = cancellation.handle();
    let execution_session = session.clone();
    session.note_operation_worker_started();
    let query_result =
        run_pgwire_blocking(Arc::clone(&cassie), "pgwire_simple_query", move |cassie| {
            cassie.execute_pgwire_sql_with_cancellation(
                &execution_session,
                (&query.0, &query.1),
                &cancellation_handle,
            )
        })
        .await;
    #[cfg(test)]
    wire_failure_test_hooks::after_worker(&cassie, session);
    drop(cancellation);
    transactions::clear_owned_idle_portals(runtime, state, session);

    match query_result {
        Ok(result) => {
            if let Some(notice) = notice {
                write_notice_response(write_half, &notice)
                    .await
                    .map_err(|_| ())?;
            }
            match write_simple_query_result(write_half, result).await {
                Ok(()) => {
                    writers::write_changed_parameter_statuses(
                        write_half,
                        session,
                        &mut state.parameter_statuses,
                    )
                    .await
                    .map_err(|_| ())?;
                    Ok(true)
                }
                Err(error) if writers::query_memory_error(&error).is_some() => {
                    transactions::fail_owned_transaction(session).await;
                    let error = writers::query_memory_error(&error).expect("query memory error");
                    write_error_response(write_half, &cassie_pg_error(error))
                        .await
                        .map(|()| false)
                        .map_err(|_| ())
                }
                Err(error) if writers::is_backend_frame_too_large(&error) => {
                    transactions::fail_owned_transaction(session).await;
                    let resource_limit = CassieError::ResourceLimit(error.to_string());
                    write_error_response(write_half, &cassie_pg_error(&resource_limit))
                        .await
                        .map(|()| false)
                        .map_err(|_| ())
                }
                Err(error) if writers::is_row_shape_mismatch(&error) => {
                    transactions::fail_owned_transaction(session).await;
                    write_error_response(write_half, &PgWireError::internal(error.to_string()))
                        .await
                        .map(|()| false)
                        .map_err(|_| ())
                }
                Err(_) => Err(()),
            }
        }
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

async fn reject_quarantined_simple_query(
    cassie: &Cassie,
    runtime: &crate::runtime::RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &crate::app::CassieSession,
    sql: &str,
) -> Option<ConnectionStep> {
    if let Err(error) = cassie.check_storage_recovery_required() {
        if crate::sql::parser::parse_terminal_transaction_control(sql).is_none() {
            return Some(
                write_simple_query_error(
                    runtime,
                    write_half,
                    state,
                    session,
                    cassie_pg_error(&error),
                )
                .await,
            );
        }
    }
    None
}
