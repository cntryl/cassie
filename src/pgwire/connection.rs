use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::str;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::pgwire::auth;
use crate::pgwire::handlers::query;
use crate::pgwire::protocol::{
    decode, encode, ClientMessage, Portal, PreparedStatement, ReadyState, RowDescriptionField,
    ServerMessage, WireError,
};
use crate::runtime::ExecutionMode;
use crate::types::Value;

const DEFAULT_STATEMENT: &str = "_pstmt_";
const PROTOCOL_VERSION_3: i32 = 0x0003_0000;
const SSL_REQUEST_CODE: i32 = 80_877_103;
const MIN_STARTUP_MESSAGE_BYTES: usize = 8;
const PASSWORD_MESSAGE_TAG: u8 = b'p';

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
    Startup(HashMap<String, String>),
}

#[derive(Debug)]
enum HandshakeError {
    Closed,
    Invalid(String),
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

    fn statement_name(name: &str) -> String {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            DEFAULT_STATEMENT.to_string()
        } else {
            trimmed.to_string()
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

    loop {
        match handshake_state {
            HandshakeState::AwaitStartup => match read_startup_frame(&mut reader).await {
                Ok(StartupFrame::SslRequest) => {
                    if write_ssl_not_supported(&mut write_half).await.is_err() {
                        break;
                    }
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
                        let session = cassie
                            .create_session(&startup_user, startup_database.clone())
                            .await;
                        state.session = Some(session);
                        state.ready = ReadyState::Idle;
                        runtime.record_pgwire_auth_ok();
                        if write_auth_ok(&mut write_half).await.is_err() {
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
                        if auth::validate_user_password(
                            &config.user,
                            &config.password,
                            &user,
                            Some(&password),
                        )
                        .is_ok()
                        {
                            state.authenticated = true;
                            let session = cassie.create_session(&user, database).await;
                            state.session = Some(session);
                            state.ready = ReadyState::Idle;
                            runtime.record_pgwire_auth_ok();
                            if write_auth_ok(&mut write_half).await.is_err() {
                                break;
                            }
                            handshake_state = HandshakeState::Ready;
                        } else {
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
                let mut line = String::new();
                let read = reader.read_line(&mut line).await;
                if read.is_err() {
                    break;
                }
                if read.ok().unwrap_or_default() == 0 {
                    break;
                }

                let msg = decode(&line);
                let mut response = Vec::new();

                match &msg {
                    ClientMessage::Query(sql) => {
                        runtime.record_pgwire_message("query");
                        runtime.record_pgwire_simple_query();
                        if !state.authenticated {
                            runtime.record_pgwire_protocol_error();
                            response.push(ServerMessage::ErrorResponse(
                                WireError::NotAuthenticated.to_string(),
                            ));
                        } else if let Some(active_session) = state.session.as_ref() {
                            let query_response =
                                query::run_simple_query(&cassie, active_session, sql, Vec::new())
                                    .await;
                            if query_response
                                .iter()
                                .any(|part| matches!(part, ServerMessage::ErrorResponse(_)))
                            {
                                runtime.record_pgwire_protocol_error();
                            }
                            response.extend(query_response);
                            response.push(ServerMessage::ReadyForQuery);
                        }
                    }
                    ClientMessage::Parse { name, query } => {
                        runtime.record_pgwire_message("parse");
                        if !state.authenticated {
                            runtime.record_pgwire_protocol_error();
                            response.push(ServerMessage::ErrorResponse(
                                WireError::NotAuthenticated.to_string(),
                            ));
                            continue;
                        }

                        match crate::sql::parser::parse_statement(query) {
                            Ok(_) => {
                                let statement_name = SessionState::statement_name(name);
                                let prepared_name = statement_name.clone();
                                let existed = state.prepared.insert(
                                    statement_name,
                                    PreparedStatement {
                                        name: prepared_name,
                                        query: query.clone(),
                                    },
                                );
                                if existed.is_none() {
                                    runtime.record_pgwire_prepared_delta(1);
                                }
                                response.push(ServerMessage::ParseComplete);
                            }
                            Err(error) => {
                                runtime.record_pgwire_protocol_error();
                                response.push(ServerMessage::ErrorResponse(error.0));
                            }
                        };
                    }
                    ClientMessage::Bind { name, params } => {
                        runtime.record_pgwire_message("bind");
                        if !state.authenticated {
                            runtime.record_pgwire_protocol_error();
                            response.push(ServerMessage::ErrorResponse(
                                WireError::NotAuthenticated.to_string(),
                            ));
                            continue;
                        }

                        let statement_name = SessionState::statement_name(name);
                        if !state.prepared.contains_key(&statement_name) {
                            runtime.record_pgwire_protocol_error();
                            response.push(ServerMessage::ErrorResponse(format!(
                                "statement '{}' is not prepared",
                                statement_name
                            )));
                            continue;
                        }
                        let existed = state.portals.insert(
                            statement_name.clone(),
                            Portal {
                                name: statement_name.clone(),
                                statement_name: statement_name.clone(),
                                limit: None,
                                params: params.clone(),
                            },
                        );
                        if existed.is_none() {
                            runtime.record_pgwire_portal_delta(1);
                        }
                        response.push(ServerMessage::BindComplete);
                    }
                    ClientMessage::Describe(name) => {
                        runtime.record_pgwire_message("describe");
                        if !state.authenticated {
                            runtime.record_pgwire_protocol_error();
                            response.push(ServerMessage::ErrorResponse(
                                WireError::NotAuthenticated.to_string(),
                            ));
                        } else {
                            let statement_name = SessionState::statement_name(name);
                            if let Some(prepared) = state.prepared.get(&statement_name) {
                                match query::describe_query(&cassie, &prepared.query).await {
                                    Ok(columns) => response.push(ServerMessage::RowDescription(
                                        columns
                                            .into_iter()
                                            .map(RowDescriptionField::from)
                                            .collect(),
                                    )),
                                    Err(error) => {
                                        runtime.record_pgwire_protocol_error();
                                        response
                                            .push(ServerMessage::ErrorResponse(error.to_string()))
                                    }
                                }
                            } else {
                                runtime.record_pgwire_protocol_error();
                                response.push(ServerMessage::ErrorResponse(format!(
                                    "statement '{}' is not prepared",
                                    statement_name
                                )));
                            }
                        }
                    }
                    ClientMessage::Execute { name, limit } => {
                        runtime.record_pgwire_message("execute");
                        runtime.record_pgwire_extended_query();
                        if !state.authenticated {
                            runtime.record_pgwire_protocol_error();
                            response.push(ServerMessage::ErrorResponse(
                                WireError::NotAuthenticated.to_string(),
                            ));
                            continue;
                        }

                        let statement_name = SessionState::statement_name(name);
                        let Some(active_session) = state.session.as_ref() else {
                            response.push(ServerMessage::ErrorResponse(
                                WireError::NotAuthenticated.to_string(),
                            ));
                            continue;
                        };
                        let prepared_query = match state.prepared.get(&statement_name) {
                            Some(prepared) => prepared.query.clone(),
                            None => {
                                runtime.record_pgwire_protocol_error();
                                response.push(ServerMessage::ErrorResponse(format!(
                                    "statement '{}' is not prepared",
                                    statement_name
                                )));
                                continue;
                            }
                        };
                        let (params, portal_limit) = match state.portals.get(&statement_name) {
                            Some(portal) => (
                                portal
                                    .params
                                    .iter()
                                    .map(|value| query::parse_bind_param(value))
                                    .collect(),
                                portal.limit,
                            ),
                            None => (Vec::new(), None),
                        };
                        let limit = limit.or(portal_limit);
                        let query_result = cassie
                            .execute_sql_with_mode(
                                active_session,
                                &prepared_query,
                                params,
                                ExecutionMode::ExtendedQuery,
                            )
                            .await;
                        match query_result {
                            Ok(mut result) => {
                                if let Some(limit) = limit {
                                    let limit = limit.max(0) as usize;
                                    result.rows = result.rows.into_iter().take(limit).collect();
                                }
                                response.push(ServerMessage::RowDescription(
                                    result
                                        .columns
                                        .into_iter()
                                        .map(RowDescriptionField::from)
                                        .collect(),
                                ));
                                for row in result.rows {
                                    response.push(ServerMessage::DataRow(
                                        row.into_iter().map(value_to_text).collect(),
                                    ));
                                }
                                response.push(ServerMessage::CommandComplete(result.command));
                            }
                            Err(error) => {
                                runtime.record_pgwire_protocol_error();
                                response.push(ServerMessage::ErrorResponse(error.to_string()))
                            }
                        };
                    }
                    ClientMessage::Close(name) => {
                        runtime.record_pgwire_message("close");
                        let statement_name = SessionState::statement_name(name);
                        if state.prepared.remove(&statement_name).is_some() {
                            runtime.record_pgwire_prepared_delta(-1);
                        }
                        if state.portals.remove(&statement_name).is_some() {
                            runtime.record_pgwire_portal_delta(-1);
                        }
                        response.push(ServerMessage::CloseComplete);
                    }
                    ClientMessage::Sync => {
                        runtime.record_pgwire_message("sync");
                        state.ready = ReadyState::Idle;
                        response.push(ServerMessage::SyncComplete);
                        response.push(ServerMessage::ReadyForQuery);
                    }
                    ClientMessage::Startup { .. } | ClientMessage::Password { .. } => {
                        runtime.record_pgwire_protocol_error();
                        response.push(ServerMessage::ErrorResponse(
                            "startup and password messages are not accepted after authentication"
                                .to_string(),
                        ));
                    }
                    ClientMessage::Unknown(text) => {
                        runtime.record_pgwire_message("unknown");
                        runtime.record_pgwire_protocol_error();
                        response.push(ServerMessage::ErrorResponse(format!(
                            "unsupported message: {text}"
                        )));
                    }
                }

                for part in response {
                    if write_half.write_all(&encode(&part)).await.is_err() {
                        return;
                    }
                    let _ = write_half.flush().await;
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
