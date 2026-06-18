use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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

#[derive(Debug)]
struct SessionState {
    session: Option<CassieSession>,
    startup_user: Option<String>,
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

    loop {
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
            ClientMessage::Startup { user, database } => {
                runtime.record_pgwire_message("startup");
                state.startup_user = Some(user.clone());
                if config.password.is_empty() {
                    state.authenticated = true;
                    state.session = Some(CassieSession::new(user.clone(), database.clone()));
                    state.ready = ReadyState::Idle;
                    runtime.record_pgwire_auth_ok();
                    response.push(ServerMessage::AuthenticationOk);
                } else {
                    response.push(ServerMessage::AuthChallenge);
                }
            }
            ClientMessage::Password { user, password } => {
                runtime.record_pgwire_message("password");
                let auth_user = if user == "postgres" {
                    state.startup_user.as_deref().unwrap_or("postgres")
                } else {
                    user.as_str()
                };

                if auth::validate_user_password(
                    &config.user,
                    &config.password,
                    auth_user,
                    Some(password),
                )
                .is_ok()
                {
                    state.authenticated = true;
                    state.session = Some(CassieSession::new(auth_user.to_string(), None));
                    state.ready = ReadyState::Idle;
                    runtime.record_pgwire_auth_ok();
                    response.push(ServerMessage::AuthenticationOk);
                } else {
                    runtime.record_pgwire_auth_failed();
                    response.push(ServerMessage::ErrorResponse(
                        WireError::NotAuthenticated.to_string(),
                    ));
                    runtime.record_pgwire_protocol_error();
                }
            }
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
                        query::run_simple_query(&cassie, active_session, sql, Vec::new()).await;
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
                    continue;
                }

                let statement_name = SessionState::statement_name(name);
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
                match query::describe_query(&cassie, &prepared_query).await {
                    Ok(columns) => response.push(ServerMessage::RowDescription(
                        columns.into_iter().map(RowDescriptionField::from).collect(),
                    )),
                    Err(error) => {
                        runtime.record_pgwire_protocol_error();
                        response.push(ServerMessage::ErrorResponse(error.to_string()))
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
