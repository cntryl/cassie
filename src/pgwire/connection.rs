use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::str;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

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
                        let _ =
                            write_error_response(&mut write_half, "FATAL", "08P01", &error).await;
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
                        "FATAL",
                        "08P01",
                        "invalid startup packet",
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
                        match cassie
                            .authenticate_role(&user, Some(&password), database.clone())
                            .await
                        {
                            Ok(session) => {
                                state.authenticated = true;
                                state.session = Some(session.clone());
                                state.ready = ReadyState::Idle;
                                runtime.record_pgwire_auth_ok();
                                if write_auth_ok(&mut write_half).await.is_err() {
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
                                    "FATAL",
                                    "28000",
                                    "authentication failed",
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                            Err(error) => {
                                runtime.record_pgwire_protocol_error();
                                if write_error_response(
                                    &mut write_half,
                                    "FATAL",
                                    "XX000",
                                    &error.to_string(),
                                )
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
                            "FATAL",
                            "08P01",
                            &format!("invalid password message: {error}"),
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
                        "ERROR",
                        "28000",
                        &WireError::NotAuthenticated.to_string(),
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
                            "ERROR",
                            "28000",
                            &WireError::NotAuthenticated.to_string(),
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
                                "ERROR",
                                "08P01",
                                &format!("invalid simple query message: {error}"),
                            )
                            .await;
                            let _ = write_ready_for_query(&mut write_half, session).await;
                            continue;
                        }
                    };

                    match cassie.execute_sql(session, &sql, Vec::new()).await {
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
                            if write_error_response(
                                &mut write_half,
                                "ERROR",
                                simple_query_error_code(&error),
                                &error.to_string(),
                            )
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
                            "ERROR",
                            "08P01",
                            &format!("invalid extended query message: {error}"),
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
                            "ERROR",
                            "0A000",
                            "COPY protocol messages are not supported",
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
                            "ERROR",
                            "0A000",
                            "function call protocol messages are not supported",
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
                                if write_error_response(
                                    &mut write_half,
                                    "ERROR",
                                    simple_query_error_code(&error),
                                    &error.to_string(),
                                )
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
                                        crate::sql::parameter_type_oids(&parsed, &parameter_types);
                                    let existed = state.prepared.insert(
                                        name.clone(),
                                        PreparedStatement {
                                            name,
                                            query,
                                            parsed,
                                            sql_fingerprint,
                                            parameter_count,
                                            parameter_types,
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
                                        "ERROR",
                                        "42601",
                                        &error.0,
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
                                    "ERROR",
                                    "26000",
                                    &format!("statement '{}' is not prepared", statement_name),
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
                                    "ERROR",
                                    "08P01",
                                    &format!(
                                        "bind message supplies {} parameters, but prepared statement '{}' requires {}",
                                        params.len(),
                                        statement_name,
                                        prepared.parameter_count
                                    ),
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
                            let prepared = match target {
                                DescribeTarget::Statement => match state.prepared.get(&name) {
                                    Some(prepared) => prepared.clone(),
                                    None => {
                                        runtime.record_pgwire_protocol_error();
                                        awaiting_sync = true;
                                        if write_error_response(
                                            &mut write_half,
                                            "ERROR",
                                            "26000",
                                            &format!("statement '{}' is not prepared", name),
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }
                                },
                                DescribeTarget::Portal => match state.portals.get(&name) {
                                    Some(portal) => {
                                        match state.prepared.get(&portal.statement_name) {
                                            Some(prepared) => prepared.clone(),
                                            None => {
                                                runtime.record_pgwire_protocol_error();
                                                awaiting_sync = true;
                                                if write_error_response(
                                                    &mut write_half,
                                                    "ERROR",
                                                    "26000",
                                                    &format!("portal '{}' is not bound", name),
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
                                    None => {
                                        runtime.record_pgwire_protocol_error();
                                        awaiting_sync = true;
                                        if write_error_response(
                                            &mut write_half,
                                            "ERROR",
                                            "26000",
                                            &format!("portal '{}' is not bound", name),
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }
                                },
                            };

                            match cassie
                                .describe_parsed_statement(
                                    prepared.parsed.clone(),
                                    prepared.sql_fingerprint,
                                )
                                .await
                            {
                                Ok(columns) => {
                                    if write_parameter_description(
                                        &mut write_half,
                                        &prepared.parameter_types,
                                    )
                                    .await
                                    .is_err()
                                    {
                                        break;
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
                                    if write_error_response(
                                        &mut write_half,
                                        "ERROR",
                                        simple_query_error_code(&error),
                                        &error.to_string(),
                                    )
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
                            let Some(portal) = state.portals.get(&portal_name) else {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                if write_error_response(
                                    &mut write_half,
                                    "ERROR",
                                    "26000",
                                    &format!("portal '{}' is not bound", portal_name),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            };
                            let Some(prepared) = state.prepared.get(&portal.statement_name) else {
                                runtime.record_pgwire_protocol_error();
                                awaiting_sync = true;
                                if write_error_response(
                                    &mut write_half,
                                    "ERROR",
                                    "26000",
                                    &format!(
                                        "statement '{}' is not prepared",
                                        portal.statement_name
                                    ),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            };

                            let query_result = cassie
                                .execute_preparsed_statement_with_mode(
                                    &session,
                                    prepared.parsed.clone(),
                                    prepared.sql_fingerprint,
                                    portal.params.clone(),
                                    ExecutionMode::ExtendedQuery,
                                )
                                .await;
                            match query_result {
                                Ok(mut result) => {
                                    if let Some(limit) = limit {
                                        let limit = limit.max(0) as usize;
                                        result.rows = result.rows.into_iter().take(limit).collect();
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
                                    if write_error_response(
                                        &mut write_half,
                                        "ERROR",
                                        simple_query_error_code(&error),
                                        &error.to_string(),
                                    )
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
                                "ERROR",
                                "08P01",
                                &format!("unsupported message: {}", char::from(tag)),
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

fn validate_startup_parameters(parameters: &HashMap<String, String>) -> Result<(), String> {
    if parameters.get("user").is_some_and(String::is_empty) {
        return Ok(());
    }

    if let Some(value) = parameters.get("user") {
        if value.trim().is_empty() {
            return Err("invalid startup option 'user'".to_string());
        }
    }

    if parameters.get("database").is_some_and(String::is_empty) {
        return Err("invalid startup option 'database'".to_string());
    }

    for key in parameters.keys() {
        if key.starts_with("_pq_") {
            return Err(format!("unsupported startup option: {key}"));
        }
        if key == "replication" {
            return Err(format!("unsupported startup option: {key}"));
        }
    }

    Ok(())
}

async fn write_auth_ok(write_half: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let mut frame = Vec::new();
    frame.push(b'R');
    frame.extend_from_slice(&8_i32.to_be_bytes());
    frame.extend_from_slice(&0_i32.to_be_bytes());
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

async fn write_auth_cleartext(write_half: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let mut frame = Vec::new();
    frame.push(b'R');
    frame.extend_from_slice(&8_i32.to_be_bytes());
    frame.extend_from_slice(&3_i32.to_be_bytes());
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

async fn write_ssl_not_supported(write_half: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    write_half.write_all(b"N").await?;
    write_half.flush().await?;
    Ok(())
}

async fn write_error_response(
    write_half: &mut (impl AsyncWrite + Unpin),
    severity: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"S");
    payload.extend_from_slice(severity.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"C");
    payload.extend_from_slice(code.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"M");
    payload.extend_from_slice(message.as_bytes());
    payload.push(0);
    payload.push(0);

    let mut frame = vec![b'E'];
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload too large"))?
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);

    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

async fn write_simple_query_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    result: crate::executor::QueryResult,
) -> io::Result<()> {
    let crate::executor::QueryResult {
        columns,
        rows,
        command,
    } = result;

    if !columns.is_empty() {
        let mut frames = Vec::new();
        append_row_description_frame(&mut frames, &columns)?;
        for row in rows {
            append_data_row_frame(&mut frames, row, &columns, &[])?;
        }
        append_command_complete_frame(&mut frames, &command)?;
        write_half.write_all(&frames).await?;
        write_half.flush().await?;
        return Ok(());
    }

    write_command_complete(write_half, &command).await?;
    Ok(())
}

async fn write_row_description(
    write_half: &mut (impl AsyncWrite + Unpin),
    columns: &[crate::executor::ColumnMeta],
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_row_description_frame(&mut frame, columns)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

fn append_row_description_frame(
    frame: &mut Vec<u8>,
    columns: &[crate::executor::ColumnMeta],
) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(columns.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many columns"))?
            .to_be_bytes(),
    );

    for column in columns {
        payload.extend_from_slice(column.name.as_bytes());
        payload.push(0);
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&0_i16.to_be_bytes());
        payload.extend_from_slice(
            &i32::try_from(column.type_oid)
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "column oid out of range")
                })?
                .to_be_bytes(),
        );
        payload.extend_from_slice(&column.typlen.to_be_bytes());
        payload.extend_from_slice(&column.atttypmod.to_be_bytes());
        payload.extend_from_slice(&column.format_code.to_be_bytes());
    }

    append_backend_frame(frame, b'T', &payload)
}

async fn write_parameter_description(
    write_half: &mut (impl AsyncWrite + Unpin),
    parameter_types: &[i32],
) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(parameter_types.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many parameters"))?
            .to_be_bytes(),
    );
    for oid in parameter_types {
        payload.extend_from_slice(&oid.to_be_bytes());
    }

    write_backend_frame(write_half, b't', &payload).await
}

async fn write_data_row(
    write_half: &mut (impl AsyncWrite + Unpin),
    row: Vec<Value>,
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_data_row_frame(&mut frame, row, columns, result_formats)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

fn append_data_row_frame(
    frame: &mut Vec<u8>,
    row: Vec<Value>,
    columns: &[crate::executor::ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    if !result_formats.is_empty()
        && result_formats.len() != 1
        && result_formats.len() != columns.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format count",
        ));
    }

    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(row.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many row values"))?
            .to_be_bytes(),
    );

    for (index, value) in row.into_iter().enumerate() {
        match value {
            Value::Null => payload.extend_from_slice(&(-1_i32).to_be_bytes()),
            other => {
                let format_code = match result_formats.len() {
                    0 => 0,
                    1 => result_formats[0],
                    _ => result_formats[index],
                };
                let bytes = if format_code == 0 {
                    value_to_text(other).into_bytes()
                } else if format_code == 1 {
                    value_to_binary(other, columns[index].type_oid)?
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsupported result format",
                    ));
                };
                payload.extend_from_slice(
                    &i32::try_from(bytes.len())
                        .map_err(|_| {
                            io::Error::new(io::ErrorKind::InvalidInput, "value too large")
                        })?
                        .to_be_bytes(),
                );
                payload.extend_from_slice(&bytes);
            }
        }
    }

    append_backend_frame(frame, b'D', &payload)
}

async fn write_command_complete(
    write_half: &mut (impl AsyncWrite + Unpin),
    command: &str,
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_command_complete_frame(&mut frame, command)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

fn append_command_complete_frame(frame: &mut Vec<u8>, command: &str) -> io::Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(command.as_bytes());
    payload.push(0);
    append_backend_frame(frame, b'C', &payload)
}

async fn write_ready_for_query(
    write_half: &mut (impl AsyncWrite + Unpin),
    session: &CassieSession,
) -> io::Result<()> {
    let status = if session.is_transaction_failed().await {
        b'E'
    } else if session.is_transaction_active().await {
        b'T'
    } else {
        b'I'
    };
    write_backend_frame(write_half, b'Z', &[status]).await
}

async fn write_backend_frame(
    write_half: &mut (impl AsyncWrite + Unpin),
    tag: u8,
    payload: &[u8],
) -> io::Result<()> {
    let mut frame = Vec::new();
    append_backend_frame(&mut frame, tag, payload)?;
    write_half.write_all(&frame).await?;
    write_half.flush().await?;
    Ok(())
}

fn append_backend_frame(frame: &mut Vec<u8>, tag: u8, payload: &[u8]) -> io::Result<()> {
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload too large"))?
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    Ok(())
}

async fn read_simple_query_message(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<String, HandshakeError> {
    let mut tag = [0u8; 1];
    reader.read_exact(&mut tag).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read query tag".to_string())
        }
    })?;
    if tag[0] != b'Q' {
        return Err(HandshakeError::Invalid(
            "not a simple query message".to_string(),
        ));
    }

    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read query length".to_string())
        }
    })?;

    let size = i32::from_be_bytes(length);
    if size < 4 {
        return Err(HandshakeError::Invalid(
            "invalid simple query frame length".to_string(),
        ));
    }
    let size = usize::try_from(size)
        .map_err(|_| HandshakeError::Invalid("invalid simple query frame length".to_string()))?;
    if size > MAX_SIMPLE_QUERY_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "simple query frame exceeds supported bounds".to_string(),
        ));
    }
    let mut payload = vec![0u8; size - 4];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read query payload".to_string())
        }
    })?;

    let mut cursor = 0usize;
    let sql = read_null_terminated(&payload, &mut cursor)?;
    if cursor != payload.len() {
        return Err(HandshakeError::Invalid(
            "invalid simple query payload".to_string(),
        ));
    }

    Ok(sql)
}

async fn read_frontend_message(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<FrontendMessage, HandshakeError> {
    let mut tag = [0u8; 1];
    reader.read_exact(&mut tag).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read frontend message tag".to_string())
        }
    })?;

    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read frontend message length".to_string())
        }
    })?;

    let size = i32::from_be_bytes(length);
    if size < 4 {
        return Err(HandshakeError::Invalid(
            "invalid frontend message length".to_string(),
        ));
    }

    let size = usize::try_from(size).map_err(|_| {
        HandshakeError::Invalid("frontend message length exceeds supported bounds".to_string())
    })?;
    if size > MAX_SIMPLE_QUERY_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "frontend message exceeds supported bounds".to_string(),
        ));
    }

    let mut payload = vec![0u8; size - 4];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read frontend payload".to_string())
        }
    })?;

    let mut cursor = 0usize;
    let message = match tag[0] {
        b'P' => {
            let name = read_null_terminated(&payload, &mut cursor)?;
            let query = read_null_terminated(&payload, &mut cursor)?;
            let parameter_count = read_frontend_i16(&payload, &mut cursor)?;
            let parameter_count = usize::try_from(parameter_count).map_err(|_| {
                HandshakeError::Invalid("invalid parse parameter count".to_string())
            })?;
            let mut parameter_types = Vec::with_capacity(parameter_count);
            for _ in 0..parameter_count {
                parameter_types.push(read_frontend_i32(&payload, &mut cursor)?);
            }
            FrontendMessage::Parse {
                name,
                query,
                parameter_types,
            }
        }
        b'B' => {
            let portal_name = read_null_terminated(&payload, &mut cursor)?;
            let statement_name = read_null_terminated(&payload, &mut cursor)?;
            let format_count = read_frontend_i16(&payload, &mut cursor)?;
            let format_count = usize::try_from(format_count)
                .map_err(|_| HandshakeError::Invalid("invalid bind format count".to_string()))?;
            let mut format_codes = Vec::with_capacity(format_count);
            for _ in 0..format_count {
                format_codes.push(read_frontend_i16(&payload, &mut cursor)?);
            }

            let parameter_count = read_frontend_i16(&payload, &mut cursor)?;
            let parameter_count = usize::try_from(parameter_count)
                .map_err(|_| HandshakeError::Invalid("invalid bind parameter count".to_string()))?;

            let mut params = Vec::with_capacity(parameter_count);
            for index in 0..parameter_count {
                let value_len = read_frontend_i32(&payload, &mut cursor)?;
                if value_len == -1 {
                    params.push(Value::Null);
                    continue;
                }
                let value_len = usize::try_from(value_len).map_err(|_| {
                    HandshakeError::Invalid("invalid bind parameter length".to_string())
                })?;
                let end = cursor
                    .checked_add(value_len)
                    .ok_or_else(|| HandshakeError::Invalid("invalid bind payload".to_string()))?;
                let value = payload
                    .get(cursor..end)
                    .ok_or_else(|| HandshakeError::Invalid("invalid bind payload".to_string()))?;

                let _format_code = match format_codes.as_slice() {
                    [] => 0,
                    [single] => *single,
                    codes if codes.len() == parameter_count => codes[index],
                    _ => {
                        return Err(HandshakeError::Invalid(
                            "unsupported bind format count".to_string(),
                        ))
                    }
                };

                let text = str::from_utf8(value).map_err(|_| {
                    HandshakeError::Invalid("invalid UTF-8 in bind parameter".to_string())
                })?;
                params.push(query::parse_bind_param(text));
                cursor = end;
            }

            let result_format_count = read_frontend_i16(&payload, &mut cursor)?;
            let result_format_count = usize::try_from(result_format_count)
                .map_err(|_| HandshakeError::Invalid("invalid result format count".to_string()))?;
            let mut result_formats = Vec::with_capacity(result_format_count);
            for _ in 0..result_format_count {
                result_formats.push(read_frontend_i16(&payload, &mut cursor)?);
            }

            FrontendMessage::Bind {
                portal_name,
                statement_name,
                params,
                result_formats,
            }
        }
        b'D' => {
            let target = match payload.get(cursor).copied() {
                Some(b'S') => {
                    cursor += 1;
                    DescribeTarget::Statement
                }
                Some(b'P') => {
                    cursor += 1;
                    DescribeTarget::Portal
                }
                Some(other) => {
                    return Err(HandshakeError::Invalid(format!(
                        "unsupported describe target '{}'",
                        char::from(other)
                    )))
                }
                None => {
                    return Err(HandshakeError::Invalid(
                        "missing describe target".to_string(),
                    ))
                }
            };
            let name = read_null_terminated(&payload, &mut cursor)?;
            FrontendMessage::Describe { target, name }
        }
        b'E' => {
            let portal_name = read_null_terminated(&payload, &mut cursor)?;
            let limit = read_frontend_i32(&payload, &mut cursor)?;
            let limit = if limit == 0 {
                None
            } else if limit < 0 {
                return Err(HandshakeError::Invalid(
                    "invalid execute row limit".to_string(),
                ));
            } else {
                Some(i64::from(limit))
            };
            FrontendMessage::Execute { portal_name, limit }
        }
        b'S' => {
            if !payload.is_empty() {
                return Err(HandshakeError::Invalid(
                    "sync message should not contain a payload".to_string(),
                ));
            }
            FrontendMessage::Sync
        }
        b'C' => {
            let target = match payload.get(cursor).copied() {
                Some(b'S') => {
                    cursor += 1;
                    CloseTarget::Statement
                }
                Some(b'P') => {
                    cursor += 1;
                    CloseTarget::Portal
                }
                Some(other) => {
                    return Err(HandshakeError::Invalid(format!(
                        "unsupported close target '{}'",
                        char::from(other)
                    )))
                }
                None => return Err(HandshakeError::Invalid("missing close target".to_string())),
            };
            let name = read_null_terminated(&payload, &mut cursor)?;
            FrontendMessage::Close { target, name }
        }
        b'd' => {
            cursor = payload.len();
            FrontendMessage::CopyData
        }
        b'c' => {
            cursor = payload.len();
            FrontendMessage::CopyDone
        }
        b'f' => {
            cursor = payload.len();
            FrontendMessage::CopyFail
        }
        b'F' => {
            cursor = payload.len();
            FrontendMessage::FunctionCall
        }
        b'H' => {
            if !payload.is_empty() {
                return Err(HandshakeError::Invalid(
                    "flush message should not contain a payload".to_string(),
                ));
            }
            FrontendMessage::Flush
        }
        b'X' => {
            if !payload.is_empty() {
                return Err(HandshakeError::Invalid(
                    "terminate message should not contain a payload".to_string(),
                ));
            }
            FrontendMessage::Terminate
        }
        other => FrontendMessage::Unknown(other),
    };

    if cursor != payload.len() {
        return Err(HandshakeError::Invalid(
            "invalid frontend message payload".to_string(),
        ));
    }

    Ok(message)
}

fn read_frontend_i16(payload: &[u8], cursor: &mut usize) -> Result<i16, HandshakeError> {
    let end = cursor
        .checked_add(2)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    let bytes: [u8; 2] = payload
        .get(*cursor..end)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?
        .try_into()
        .map_err(|_| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    *cursor = end;
    Ok(i16::from_be_bytes(bytes))
}

fn read_frontend_i32(payload: &[u8], cursor: &mut usize) -> Result<i32, HandshakeError> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    let bytes: [u8; 4] = payload
        .get(*cursor..end)
        .ok_or_else(|| HandshakeError::Invalid("invalid frontend payload".to_string()))?
        .try_into()
        .map_err(|_| HandshakeError::Invalid("invalid frontend payload".to_string()))?;
    *cursor = end;
    Ok(i32::from_be_bytes(bytes))
}

fn simple_query_error_code(error: &crate::app::CassieError) -> &'static str {
    match error {
        crate::app::CassieError::Parse(_) => "42601",
        crate::app::CassieError::Unauthorized => "28000",
        crate::app::CassieError::CollectionNotFound(_) | crate::app::CassieError::NotFound(_) => {
            "42P01"
        }
        crate::app::CassieError::Unsupported(_) => "0A000",
        crate::app::CassieError::InvalidVector(_)
        | crate::app::CassieError::InvalidEmbedding(_) => "22000",
        crate::app::CassieError::EmbeddingUnavailable(_) => "58030",
        crate::app::CassieError::Storage(_)
        | crate::app::CassieError::StorageBootstrap(_)
        | crate::app::CassieError::StorageMissingFamily(_)
        | crate::app::CassieError::StorageRetryable(_)
        | crate::app::CassieError::Planner(_)
        | crate::app::CassieError::Execution(_) => "XX000",
    }
}

async fn read_startup_frame(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<StartupFrame, HandshakeError> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read startup frame".to_string())
        }
    })?;

    let size = i32::from_be_bytes(length);
    if size < 0 {
        return Err(HandshakeError::Invalid(
            "negative startup frame size".to_string(),
        ));
    }
    let size = usize::try_from(size).map_err(|_| {
        HandshakeError::Invalid("startup frame size exceeds supported bounds".to_string())
    })?;

    if size < MIN_STARTUP_MESSAGE_BYTES {
        return Err(HandshakeError::Invalid(
            "startup frame too small".to_string(),
        ));
    }

    let mut payload = vec![0u8; size - 4];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read startup payload".to_string())
        }
    })?;

    let code = i32::from_be_bytes(
        *payload
            .first_chunk::<4>()
            .ok_or_else(|| HandshakeError::Invalid("malformed startup frame".to_string()))?,
    );

    if size == 8 && code == SSL_REQUEST_CODE {
        return Ok(StartupFrame::SslRequest);
    }

    if size == 16 && code == CANCEL_REQUEST_CODE {
        return Ok(StartupFrame::CancelRequest);
    }

    if code != PROTOCOL_VERSION_3 {
        return Err(HandshakeError::Invalid(
            "unsupported protocol version".to_string(),
        ));
    }

    let parameters = parse_startup_payload(&payload[4..])?;
    Ok(StartupFrame::Startup(parameters))
}

async fn read_password_message(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<String, HandshakeError> {
    let mut header = [0u8; 5];
    reader.read_exact(&mut header).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read password message header".to_string())
        }
    })?;
    if header[0] != PASSWORD_MESSAGE_TAG {
        return Err(HandshakeError::Invalid(
            "not a password message".to_string(),
        ));
    }

    let payload_len = i32::from_be_bytes(header[1..].try_into().unwrap_or([0u8; 4]));
    if payload_len < 4 {
        return Err(HandshakeError::Invalid(
            "invalid password frame length".to_string(),
        ));
    }
    let payload_len = usize::try_from(payload_len)
        .map_err(|_| HandshakeError::Invalid("invalid password frame length".to_string()))?
        - 4;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Invalid("failed to read password payload".to_string())
        }
    })?;

    let mut cursor = 0usize;
    let value = read_null_terminated(&payload, &mut cursor)?;
    if cursor != payload.len() {
        return Err(HandshakeError::Invalid(
            "invalid password payload".to_string(),
        ));
    }
    Ok(value)
}

fn parse_startup_payload(payload: &[u8]) -> Result<HashMap<String, String>, HandshakeError> {
    let mut cursor = 0usize;
    let mut parameters = HashMap::new();
    while cursor < payload.len() {
        let key = read_null_terminated(payload, &mut cursor)?;
        if key.is_empty() {
            if cursor == payload.len() {
                break;
            }
            return Err(HandshakeError::Invalid(
                "malformed startup payload: unexpected trailing data".to_string(),
            ));
        }

        let value = read_null_terminated(payload, &mut cursor)?;
        parameters.insert(key, value);
    }

    Ok(parameters)
}

fn read_null_terminated(payload: &[u8], cursor: &mut usize) -> Result<String, HandshakeError> {
    let remaining = payload
        .get(*cursor..)
        .ok_or_else(|| HandshakeError::Invalid("invalid payload cursor".to_string()))?;

    let end = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| HandshakeError::Invalid("missing null terminator".to_string()))?;

    let decoded = str::from_utf8(&remaining[..end])
        .map_err(|_| HandshakeError::Invalid("invalid UTF-8 in startup option".to_string()))?;

    *cursor += end + 1;
    Ok(decoded.to_string())
}

fn value_to_text(value: Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Float64(v) => v.to_string(),
        Value::String(v) => v,
        Value::Vector(v) => format!(
            "[{}]",
            v.values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(v) => v.to_string(),
    }
}

fn value_to_binary(value: Value, type_oid: i64) -> io::Result<Vec<u8>> {
    match type_oid {
        16 => match value {
            Value::Bool(v) => Ok(vec![if v { 1 } else { 0 }]),
            other => Ok(value_to_text(other).into_bytes()),
        },
        17 => match value {
            Value::String(v) => {
                let bytes = decode_bytea(&v).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "cannot encode BYTEA value to binary",
                    )
                })?;
                Ok(bytes)
            }
            other => Ok(value_to_text(other).into_bytes()),
        },
        21 => match value {
            Value::Int64(v) => Ok((v as i16).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        20 => match value {
            Value::Int64(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Float64(v) => Ok((v as i64).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        23 => match value {
            Value::Int64(v) => Ok((v as i32).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        701 => match value {
            Value::Float64(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Int64(v) => Ok((v as f64).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        25 | 1042 | 1043 | 705 | 114 => Ok(value_to_text(value).into_bytes()),
        _ => Ok(value_to_text(value).into_bytes()),
    }
}

fn decode_bytea(value: &str) -> io::Result<Vec<u8>> {
    if !value.starts_with("\\x") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bytea value must be hex format '\\x...'",
        ));
    }
    if value.len() == 2 {
        return Ok(Vec::new());
    }
    if (value.len() - 2).rem_euclid(2) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bytea value must have an even number of hex digits",
        ));
    }

    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity((value.len() - 2) / 2);
    let mut index = 2;
    while index < value.len() {
        let high = decode_hex_digit(bytes[index]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "bytea value must be hexadecimal",
            )
        })?;
        let low = decode_hex_digit(bytes[index + 1]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "bytea value must be hexadecimal",
            )
        })?;
        out.push((high << 4) | low);
        index += 2;
    }

    Ok(out)
}

fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
