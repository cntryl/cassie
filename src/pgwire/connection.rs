use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::str;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::task;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::pgwire::handlers::query;
use crate::pgwire::protocol::{Portal, PreparedStatement, ReadyState, WireError};
use crate::runtime::ExecutionMode;
use crate::types::Value;

const PROTOCOL_VERSION_3: i32 = 0x0003_0000;
const SSL_REQUEST_CODE: i32 = 80_877_103;
const CANCEL_REQUEST_CODE: i32 = 80_877_102;
const MIN_STARTUP_MESSAGE_BYTES: usize = 8;
const PASSWORD_MESSAGE_TAG: u8 = b'p';
const MAX_SIMPLE_QUERY_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
enum HandshakeState {
    AwaitStartup,
    AwaitPassword {
        user: String,
        database: Option<String>,
    },
    Ready,
}

#[derive(Debug)]
enum StartupFrame {
    SslRequest,
    CancelRequest,
    Startup(HashMap<String, String>),
}

#[derive(Debug)]
enum HandshakeError {
    Closed,
    Invalid(String),
}

#[derive(Debug)]
enum FrontendMessage {
    Parse {
        name: String,
        query: String,
        parameter_types: Vec<i32>,
    },
    Bind {
        portal_name: String,
        statement_name: String,
        params: Vec<Value>,
        result_formats: Vec<i16>,
    },
    Describe {
        target: DescribeTarget,
        name: String,
    },
    Execute {
        portal_name: String,
        limit: Option<i64>,
    },
    Close {
        target: CloseTarget,
        name: String,
    },
    CopyData,
    CopyDone,
    CopyFail,
    FunctionCall,
    Sync,
    Flush,
    Terminate,
    Unknown(u8),
}

#[derive(Debug)]
enum DescribeTarget {
    Statement,
    Portal,
}

#[derive(Debug)]
enum CloseTarget {
    Statement,
    Portal,
}

#[derive(Debug)]
struct SessionState {
    session: Option<CassieSession>,
    startup_user: Option<String>,
    startup_database: Option<String>,
    authenticated: bool,
    ready: ReadyState,
    prepared: HashMap<String, PreparedStatement>,
    portals: HashMap<String, Portal>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            session: None,
            startup_user: None,
            startup_database: None,
            authenticated: false,
            ready: ReadyState::InTransaction,
            prepared: HashMap::new(),
            portals: HashMap::new(),
        }
    }
}

#[path = "connection/codecs.rs"]
mod codecs;
#[path = "connection/errors.rs"]
mod errors;
#[path = "connection/readers.rs"]
mod readers;
#[path = "connection/startup_params.rs"]
mod startup_params;
#[path = "connection/writers.rs"]
mod writers;

use errors::{PgWireError, PgWireSeverity};
use readers::*;
use startup_params::validate_startup_parameters;
use writers::*;

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
        match handshake_state {
            HandshakeState::AwaitStartup => match read_startup_frame(&mut reader).await {
                Ok(StartupFrame::SslRequest) => {
                    if write_ssl_not_supported(&mut write_half).await.is_err() {
                        break;
                    }
                }
                Ok(StartupFrame::CancelRequest) => break,
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
                        let _ = write_error_response(
                            &mut write_half,
                            &PgWireError::fatal_protocol(error),
                        )
                        .await;
                        continue;
                    }

                    let startup_user = state
                        .startup_user
                        .clone()
                        .unwrap_or_else(|| config.user.clone());
                    let startup_database = state.startup_database.clone();

                    if config.password.is_empty() {
                        state.authenticated = true;
                        let session =
                            cassie.create_session(&startup_user, startup_database.clone());
                        state.session = Some(session.clone());
                        state.ready = ReadyState::Idle;
                        runtime.record_pgwire_auth_ok();
                        if write_auth_ok(&mut write_half).await.is_err() {
                            break;
                        }
                        if write_parameter_statuses(&mut write_half).await.is_err() {
                            break;
                        }
                        if write_ready_for_query(&mut write_half, &session)
                            .await
                            .is_err()
                        {
                            break;
                        }
                        handshake_state = HandshakeState::Ready;
                    } else {
                        if write_auth_cleartext(&mut write_half).await.is_err() {
                            break;
                        }
                        handshake_state = HandshakeState::AwaitPassword {
                            user: startup_user,
                            database: startup_database,
                        };
                    }
                }
                Err(HandshakeError::Closed) => {
                    break;
                }
                Err(HandshakeError::Invalid(_)) => {
                    runtime.record_pgwire_protocol_error();
                    let _ = write_error_response(
                        &mut write_half,
                        &PgWireError::fatal_protocol("invalid startup packet"),
                    )
                    .await;
                }
            },
            HandshakeState::AwaitPassword {
                ref user,
                ref database,
            } => {
                let user = user.clone();
                let database = database.clone();
                match read_password_message(&mut reader).await {
                    Ok(password) => {
                        runtime.record_pgwire_message("password");
                        let auth_result =
                            run_pgwire_blocking(cassie.clone(), "pgwire_auth", move |cassie| {
                                cassie.authenticate_role(&user, Some(&password), database.clone())
                            })
                            .await;

                        match auth_result {
                            Ok(session) => {
                                state.authenticated = true;
                                state.session = Some(session.clone());
                                state.ready = ReadyState::Idle;
                                runtime.record_pgwire_auth_ok();
                                if write_auth_ok(&mut write_half).await.is_err() {
                                    break;
                                }
                                if write_parameter_statuses(&mut write_half).await.is_err() {
                                    break;
                                }
                                if write_ready_for_query(&mut write_half, &session)
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                handshake_state = HandshakeState::Ready;
                            }
                            Err(CassieError::Unauthorized) => {
                                runtime.record_pgwire_auth_failed();
                                runtime.record_pgwire_protocol_error();
                                if write_error_response(
                                    &mut write_half,
                                    &PgWireError::auth_failed("authentication failed"),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                            Err(error) => {
                                runtime.record_pgwire_protocol_error();
                                let pg_error =
                                    PgWireError::from_cassie_error(PgWireSeverity::Fatal, &error);
                                if write_error_response(&mut write_half, &pg_error)
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Err(HandshakeError::Closed) => {
                        break;
                    }
                    Err(HandshakeError::Invalid(error)) => {
                        runtime.record_pgwire_protocol_error();
                        let _ = write_error_response(
                            &mut write_half,
                            &PgWireError::fatal_protocol(format!(
                                "invalid password message: {error}"
                            )),
                        )
                        .await;
                    }
                }
            }
            HandshakeState::Ready => {
                let next_tag = match reader.fill_buf().await {
                    Ok(buffer) => {
                        if buffer.is_empty() {
                            break;
                        }
                        buffer[0]
                    }
                    Err(_) => break,
                };

                let Some(session) = state.session.as_ref().cloned() else {
                    runtime.record_pgwire_protocol_error();
                    let _ = write_error_response(
                        &mut write_half,
                        &PgWireError::auth_required(WireError::NotAuthenticated.to_string()),
                    )
                    .await;
                    continue;
                };

                if awaiting_sync {
                    if next_tag == b'Q' {
                        match read_simple_query_message(&mut reader).await {
                            Ok(_) => {}
                            Err(HandshakeError::Closed) => break,
                            Err(HandshakeError::Invalid(_)) => continue,
                        }
                        continue;
                    }

                    let message = match read_frontend_message(&mut reader).await {
                        Ok(message) => message,
                        Err(HandshakeError::Closed) => break,
                        Err(HandshakeError::Invalid(_)) => continue,
                    };

                    match message {
                        FrontendMessage::Sync => {
                            runtime.record_pgwire_message("sync");
                            awaiting_sync = false;
                            state.ready = ReadyState::Idle;
                            if write_ready_for_query(&mut write_half, &session)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        FrontendMessage::Terminate => break,
                        FrontendMessage::Flush => {
                            runtime.record_pgwire_message("flush");
                        }
                        _ => {}
                    }
                    continue;
                }

                if next_tag == b'Q' {
                    runtime.record_pgwire_message("query");
                    runtime.record_pgwire_simple_query();

                    let session = if let Some(active_session) = state.session.as_ref() {
                        active_session
                    } else {
                        runtime.record_pgwire_protocol_error();
                        let _ = write_error_response(
                            &mut write_half,
                            &PgWireError::auth_required(WireError::NotAuthenticated.to_string()),
                        )
                        .await;
                        continue;
                    };

                    let sql = match read_simple_query_message(&mut reader).await {
                        Ok(sql) => sql,
                        Err(HandshakeError::Closed) => break,
                        Err(HandshakeError::Invalid(error)) => {
                            runtime.record_pgwire_protocol_error();
                            let _ = write_error_response(
                                &mut write_half,
                                &PgWireError::protocol(format!(
                                    "invalid simple query message: {error}"
                                )),
                            )
                            .await;
                            let _ = write_ready_for_query(&mut write_half, session).await;
                            continue;
                        }
                    };

                    let session_for_query = session.clone();
                    let sql_for_query = sql.clone();
                    let query_result =
                        run_pgwire_blocking(cassie.clone(), "pgwire_simple_query", move |cassie| {
                            cassie.execute_sql(&session_for_query, &sql_for_query, Vec::new())
                        })
                        .await;

                    match query_result {
                        Ok(result) => {
                            if write_simple_query_result(&mut write_half, result)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            runtime.record_pgwire_protocol_error();
                            let pg_error = cassie_pg_error(&error);
                            if write_error_response(&mut write_half, &pg_error)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }

                    if write_ready_for_query(&mut write_half, session)
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

                let message = match read_frontend_message(&mut reader).await {
                    Ok(message) => message,
                    Err(HandshakeError::Closed) => break,
                    Err(HandshakeError::Invalid(error)) => {
                        runtime.record_pgwire_protocol_error();
                        let _ = write_error_response(
                            &mut write_half,
                            &PgWireError::protocol(format!(
                                "invalid extended query message: {error}"
                            )),
                        )
                        .await;
                        continue;
                    }
                };

                match message {
                    FrontendMessage::Flush => {
                        runtime.record_pgwire_message("flush");
                        continue;
                    }
                    FrontendMessage::Terminate => break,
                    FrontendMessage::CopyData
                    | FrontendMessage::CopyDone
                    | FrontendMessage::CopyFail => {
                        runtime.record_pgwire_message("copy");
                        runtime.record_pgwire_protocol_error();
                        awaiting_sync = true;
                        if write_error_response(
                            &mut write_half,
                            &PgWireError::unsupported("COPY protocol messages are not supported"),
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    FrontendMessage::FunctionCall => {
                        runtime.record_pgwire_message("function_call");
                        runtime.record_pgwire_protocol_error();
                        awaiting_sync = true;
                        if write_error_response(
                            &mut write_half,
                            &PgWireError::unsupported(
                                "function call protocol messages are not supported",
                            ),
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    other => match other {
                        FrontendMessage::Parse {
                            name,
                            query,
                            parameter_types,
                        } => {
                            runtime.record_pgwire_message("parse");
                            if let Some(error) = crate::app::unsupported_sql_error(&query) {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                let pg_error = cassie_pg_error(&error);
                                if write_error_response(&mut write_half, &pg_error)
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                            runtime.record_sql_parse();
                            match crate::sql::parser::parse_statement(&query) {
                                Ok(parsed) => {
                                    let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
                                    let parameter_count = crate::sql::parameter_count(&parsed);
                                    let parameter_types =
                                        crate::sql::parameter_type_oids_with_catalog(
                                            &parsed,
                                            &parameter_types,
                                            &cassie.catalog,
                                        );
                                    let existed = state.prepared.insert(
                                        name.clone(),
                                        PreparedStatement {
                                            name,
                                            query,
                                            parsed,
                                            sql_fingerprint,
                                            parameter_count,
                                            parameter_types,
                                            described: false,
                                        },
                                    );
                                    if existed.is_none() {
                                        runtime.record_pgwire_prepared_delta(1);
                                    }
                                    if write_backend_frame(&mut write_half, b'1', &[])
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    runtime.record_pgwire_protocol_error();
                                    awaiting_sync = true;
                                    if write_error_response(
                                        &mut write_half,
                                        &PgWireError::new(PgWireSeverity::Error, "42601", error.0),
                                    )
                                    .await
                                    .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                        FrontendMessage::Bind {
                            portal_name,
                            statement_name,
                            params,
                            result_formats,
                        } => {
                            runtime.record_pgwire_message("bind");
                            let Some(prepared) = state.prepared.get(&statement_name) else {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                if write_error_response(
                                    &mut write_half,
                                    &PgWireError::invalid_statement(&statement_name),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            };
                            if params.len() != prepared.parameter_count {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                if write_error_response(
                                    &mut write_half,
                                    &PgWireError::protocol(format!(
                                        "bind message supplies {} parameters, but prepared statement '{}' requires {}",
                                        params.len(),
                                        statement_name,
                                        prepared.parameter_count
                                    )),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            }

                            let existed = state.portals.insert(
                                portal_name.clone(),
                                Portal {
                                    name: portal_name,
                                    statement_name,
                                    params,
                                    result_formats,
                                    described: false,
                                },
                            );
                            if existed.is_none() {
                                runtime.record_pgwire_portal_delta(1);
                            }
                            if write_backend_frame(&mut write_half, b'2', &[])
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        FrontendMessage::Describe { target, name } => {
                            runtime.record_pgwire_message("describe");
                            let describe_statement = matches!(&target, DescribeTarget::Statement);
                            let prepared = match target {
                                DescribeTarget::Statement => match state.prepared.get_mut(&name) {
                                    Some(prepared) => {
                                        prepared.described = true;
                                        prepared.clone()
                                    }
                                    None => {
                                        runtime.record_pgwire_protocol_error();
                                        awaiting_sync = true;
                                        if write_error_response(
                                            &mut write_half,
                                            &PgWireError::invalid_statement(&name),
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }
                                },
                                DescribeTarget::Portal => {
                                    let statement_name = match state.portals.get_mut(&name) {
                                        Some(portal) => {
                                            portal.described = true;
                                            portal.statement_name.clone()
                                        }
                                        None => {
                                            runtime.record_pgwire_protocol_error();
                                            awaiting_sync = true;
                                            if write_error_response(
                                                &mut write_half,
                                                &PgWireError::invalid_portal(&name),
                                            )
                                            .await
                                            .is_err()
                                            {
                                                break;
                                            }
                                            continue;
                                        }
                                    };
                                    match state.prepared.get(&statement_name) {
                                        Some(prepared) => prepared.clone(),
                                        None => {
                                            runtime.record_pgwire_protocol_error();
                                            awaiting_sync = true;
                                            if write_error_response(
                                                &mut write_half,
                                                &PgWireError::invalid_portal(&name),
                                            )
                                            .await
                                            .is_err()
                                            {
                                                break;
                                            }
                                            continue;
                                        }
                                    }
                                }
                            };

                            let parsed = prepared.parsed.clone();
                            let sql_fingerprint = prepared.sql_fingerprint;
                            let describe_result = run_pgwire_blocking(
                                cassie.clone(),
                                "pgwire_describe",
                                move |cassie| {
                                    cassie.describe_parsed_statement(parsed, sql_fingerprint)
                                },
                            )
                            .await;

                            match describe_result {
                                Ok(columns) => {
                                    if describe_statement {
                                        if write_parameter_description(
                                            &mut write_half,
                                            &prepared.parameter_types,
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    if columns.is_empty() {
                                        if write_backend_frame(&mut write_half, b'n', &[])
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    } else if write_row_description(&mut write_half, &columns)
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    runtime.record_pgwire_protocol_error();
                                    awaiting_sync = true;
                                    let pg_error = cassie_pg_error(&error);
                                    if write_error_response(&mut write_half, &pg_error)
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                        FrontendMessage::Execute { portal_name, limit } => {
                            runtime.record_pgwire_message("execute");
                            runtime.record_pgwire_extended_query();
                            let Some(portal) = state.portals.get(&portal_name).cloned() else {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                if write_error_response(
                                    &mut write_half,
                                    &PgWireError::invalid_portal(&portal_name),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            };
                            let Some(prepared) =
                                state.prepared.get(&portal.statement_name).cloned()
                            else {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                if write_error_response(
                                    &mut write_half,
                                    &PgWireError::invalid_statement(&portal.statement_name),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            };

                            let session_for_execute = session.clone();
                            let query_parsed = prepared.parsed.clone();
                            let query_sql_fingerprint = prepared.sql_fingerprint;
                            let query_params = portal.params.clone();
                            let should_describe_execute = !prepared.described && !portal.described;
                            let query_result = run_pgwire_blocking(
                                cassie.clone(),
                                "pgwire_execute",
                                move |cassie| {
                                    cassie.execute_preparsed_statement_with_mode(
                                        &session_for_execute,
                                        query_parsed,
                                        query_sql_fingerprint,
                                        query_params,
                                        ExecutionMode::ExtendedQuery,
                                    )
                                },
                            )
                            .await;

                            match query_result {
                                Ok(mut result) => {
                                    if let Some(limit) = limit {
                                        let limit = limit.max(0) as usize;
                                        result.rows = result.rows.into_iter().take(limit).collect();
                                    }
                                    if should_describe_execute && !result.columns.is_empty() {
                                        if write_row_description(&mut write_half, &result.columns)
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    for row in result.rows {
                                        if write_data_row(
                                            &mut write_half,
                                            row,
                                            &result.columns,
                                            &portal.result_formats,
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    if write_command_complete(&mut write_half, &result.command)
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    runtime.record_pgwire_protocol_error();
                                    awaiting_sync = true;
                                    let pg_error = cassie_pg_error(&error);
                                    if write_error_response(&mut write_half, &pg_error)
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                        FrontendMessage::Close { target, name } => {
                            runtime.record_pgwire_message("close");
                            match target {
                                CloseTarget::Statement => {
                                    if state.prepared.remove(&name).is_some() {
                                        runtime.record_pgwire_prepared_delta(-1);
                                    }
                                    let removed_portals = state
                                        .portals
                                        .iter()
                                        .filter(|(_, portal)| portal.statement_name == name)
                                        .map(|(portal_name, _)| portal_name.clone())
                                        .collect::<Vec<_>>();
                                    for portal_name in removed_portals {
                                        if state.portals.remove(&portal_name).is_some() {
                                            runtime.record_pgwire_portal_delta(-1);
                                        }
                                    }
                                }
                                CloseTarget::Portal => {
                                    if state.portals.remove(&name).is_some() {
                                        runtime.record_pgwire_portal_delta(-1);
                                    }
                                }
                            }
                            if write_backend_frame(&mut write_half, b'3', &[])
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        FrontendMessage::Sync => {
                            runtime.record_pgwire_message("sync");
                            state.ready = ReadyState::Idle;
                            if write_ready_for_query(&mut write_half, &session)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        FrontendMessage::Unknown(tag) => {
                            runtime.record_pgwire_message("unknown");
                            runtime.record_pgwire_protocol_error();
                            awaiting_sync = true;
                            if write_error_response(
                                &mut write_half,
                                &PgWireError::protocol(format!(
                                    "unsupported message: {}",
                                    char::from(tag)
                                )),
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                        FrontendMessage::Flush | FrontendMessage::Terminate => unreachable!(),
                        FrontendMessage::CopyData
                        | FrontendMessage::CopyDone
                        | FrontendMessage::CopyFail
                        | FrontendMessage::FunctionCall => unreachable!(),
                    },
                }
            }
        }
    }
}

async fn run_pgwire_blocking<T>(
    cassie: Arc<Cassie>,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>) -> Result<T, CassieError> + Send + 'static,
) -> Result<T, CassieError>
where
    T: Send + 'static,
{
    let runtime = cassie.runtime.clone();
    let started_at = Instant::now();
    runtime.record_pgwire_boundary_started(operation_name);

    let result = task::spawn_blocking(move || operation(cassie)).await;

    match result {
        Ok(result) => match result {
            Ok(value) => {
                runtime.record_pgwire_boundary_completed(operation_name, started_at.elapsed());
                Ok(value)
            }
            Err(error) => {
                runtime.record_pgwire_boundary_error(operation_name, started_at.elapsed());
                Err(error)
            }
        },
        Err(error) => {
            runtime.record_pgwire_boundary_join_failed(operation_name, started_at.elapsed());
            Err(CassieError::StorageRetryable(format!(
                "pgwire blocking boundary '{operation_name}' failed: {error}"
            )))
        }
    }
}

fn cassie_pg_error(error: &CassieError) -> PgWireError {
    PgWireError::from_cassie_error(PgWireSeverity::Error, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::{ColumnMeta, QueryResult};
    use crate::types::{DataType, Value};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[derive(Default)]
    struct CountingWrite {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl AsyncWrite for CountingWrite {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.bytes.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.flushes += 1;
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn should_flush_pgwire_simple_query_result_once_for_multiple_rows() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let result = QueryResult {
            columns: vec![ColumnMeta::from_data_type("id", DataType::Text)],
            rows: vec![
                vec![Value::String("doc-1".to_string())],
                vec![Value::String("doc-2".to_string())],
            ],
            command: "SELECT".to_string(),
        };

        runtime.block_on(async {
            let mut writer = CountingWrite::default();

            // Act
            write_simple_query_result(&mut writer, result)
                .await
                .expect("write simple query result");

            // Assert
            assert_eq!(writer.flushes, 1);
            assert_eq!(writer.bytes[0], b'T');
            assert!(writer.bytes.contains(&b'D'));
            assert!(writer.bytes.contains(&b'C'));
        });
    }
}
