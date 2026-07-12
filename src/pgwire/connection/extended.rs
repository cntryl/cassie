use std::io;
use std::str;
use std::sync::Arc;

use tokio::io::{AsyncWrite, AsyncWriteExt, BufReader};

use super::blocking::run_pgwire_blocking;
use super::errors::{PgWireError, PgWireSeverity};
use super::readers::read_frontend_message;
use super::state::{DescribeTarget, FrontendMessage, HandshakeError, HandshakeState, SessionState};
use super::writers::{
    append_command_complete_frame, append_data_row_frame, append_portal_suspended_frame,
    append_row_description_frame, write_bind_complete, write_close_complete, write_error_response,
    write_no_data, write_parameter_description, write_parse_complete, write_ready_for_query,
};
use super::ConnectionStep;
use crate::app::{unsupported_sql_error, Cassie, CassieError, CassieSession};
use crate::executor::{ColumnMeta, QueryResult};
use crate::pgwire::protocol::{Portal, PortalSuspended, PreparedStatement};
use crate::runtime::{ExecutionMode, RuntimeState};
use crate::types::Value;

const OID_BOOL: i32 = 16;
const OID_BYTEA: i32 = 17;
const OID_INT8: i32 = 20;
const OID_INT2: i32 = 21;
const OID_INT4: i32 = 23;
const OID_TEXT: i32 = 25;
const OID_JSON: i32 = 114;
const OID_FLOAT8: i32 = 701;
const OID_UNKNOWN: i32 = 705;
const OID_BPCHAR: i32 = 1042;
const OID_VARCHAR: i32 = 1043;
const OID_UUID: i32 = 2950;

pub(super) async fn handle_frontend_message(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    awaiting_sync: &mut bool,
    session: &CassieSession,
) -> ConnectionStep {
    let message = match read_frontend_message(reader).await {
        Ok(message) => message,
        Err(HandshakeError::Closed) => return ConnectionStep::Break,
        Err(HandshakeError::Invalid(error)) => {
            runtime.record_pgwire_protocol_error();
            let _ = write_error_response(
                write_half,
                &PgWireError::protocol(format!("invalid frontend message: {error}")),
            )
            .await;
            *awaiting_sync = true;
            return ConnectionStep::Continue(HandshakeState::Ready);
        }
    };

    match dispatch_message(cassie, runtime, write_half, state, session, message).await {
        Ok(DispatchOutcome::Continue) => ConnectionStep::Continue(HandshakeState::Ready),
        Ok(DispatchOutcome::Break) => ConnectionStep::Break,
        Err(error) => {
            if error.record_protocol_error {
                runtime.record_pgwire_protocol_error();
            }
            let _ = write_error_response(write_half, error.pg_error.as_ref()).await;
            *awaiting_sync = true;
            ConnectionStep::Continue(HandshakeState::Ready)
        }
    }
}

enum DispatchOutcome {
    Continue,
    Break,
}

async fn dispatch_message(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    message: FrontendMessage,
) -> Result<DispatchOutcome, ExtendedQueryError> {
    match message {
        FrontendMessage::Parse {
            name,
            query,
            parameter_type_oids,
        } => {
            runtime.record_pgwire_message("parse");
            handle_parse(
                cassie,
                runtime,
                write_half,
                state,
                session,
                ParseRequest {
                    name,
                    query,
                    parameter_type_oids,
                },
            )
            .await?;
        }
        FrontendMessage::Bind {
            portal,
            statement,
            parameter_formats,
            parameters,
            result_formats,
        } => {
            runtime.record_pgwire_message("bind");
            let bind = BindRequest {
                portal,
                statement,
                parameter_formats,
                parameters,
                result_formats,
            };
            handle_bind(runtime, write_half, state, bind).await?;
        }
        FrontendMessage::Describe { target, name } => {
            runtime.record_pgwire_message("describe");
            handle_describe(cassie, write_half, state, target, name).await?;
        }
        FrontendMessage::Execute { portal, max_rows } => {
            runtime.record_pgwire_message("execute");
            handle_execute(
                cassie, runtime, write_half, state, session, portal, max_rows,
            )
            .await?;
        }
        FrontendMessage::Close { target, name } => {
            runtime.record_pgwire_message("close");
            handle_close(runtime, write_half, state, target, name).await?;
        }
        FrontendMessage::Flush => {
            runtime.record_pgwire_message("flush");
            let _ = write_half.flush().await;
        }
        FrontendMessage::Sync => {
            runtime.record_pgwire_message("sync");
            let _ = write_ready_for_query(write_half, session).await;
        }
        FrontendMessage::Terminate => return Ok(DispatchOutcome::Break),
        FrontendMessage::CopyData(_) | FrontendMessage::CopyDone | FrontendMessage::CopyFail(_) => {
            return Err(ExtendedQueryError::unsupported(
                "COPY sub-protocol is not active for this connection",
            ));
        }
        FrontendMessage::FunctionCall => {
            return Err(ExtendedQueryError::unsupported(
                "function call protocol messages are not supported",
            ));
        }
        FrontendMessage::Unknown => {
            return Err(ExtendedQueryError::protocol(
                "unknown frontend message type",
            ));
        }
    }

    Ok(DispatchOutcome::Continue)
}

struct ParseRequest {
    name: String,
    query: String,
    parameter_type_oids: Vec<i32>,
}

async fn handle_parse(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    request: ParseRequest,
) -> Result<(), ExtendedQueryError> {
    let ParseRequest {
        name,
        query,
        parameter_type_oids,
    } = request;
    if !name.is_empty() && state.prepared_statements.contains_key(&name) {
        return Err(ExtendedQueryError::protocol(format!(
            "prepared statement '{name}' already exists"
        )));
    }
    if parameter_type_oids.iter().any(|oid| *oid < 0) {
        return Err(ExtendedQueryError::protocol(
            "parse parameter type OID cannot be negative",
        ));
    }
    if let Some(error) = unsupported_sql_error(&query) {
        if session.is_authenticated_read_only() {
            return Err(ExtendedQueryError::cassie(
                &CassieError::InsufficientPrivilege,
            ));
        }
        return Err(ExtendedQueryError::cassie(&error));
    }

    runtime.record_sql_parse();
    let parsed = crate::sql::parse_statement(&query).map_err(CassieError::from)?;
    session
        .authorize_statement(&parsed.statement)
        .map_err(|error| ExtendedQueryError::cassie(&error))?;
    let parameter_type_oids = normalize_parameter_type_oids(&parameter_type_oids);
    let parameter_types = crate::sql::parameter_type_oids_with_catalog(
        &parsed,
        &parameter_type_oids,
        &cassie.catalog,
    );
    let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let prepared_id = state.next_prepared_id();
    let replaced_prepared = state
        .prepared_statements
        .get(&name)
        .map(|statement| statement.id);
    if let Some(prepared_id) = replaced_prepared {
        remove_portals_for_prepared_id(state, runtime, prepared_id);
    }

    state.prepared_statements.insert(
        name.clone(),
        PreparedStatement {
            id: prepared_id,
            name,
            query,
            parsed,
            sql_fingerprint,
            parameter_count: parameter_types.len(),
            parameter_types,
            described: false,
        },
    );
    if replaced_prepared.is_none() {
        runtime.record_pgwire_prepared_delta(1);
    }

    let _ = write_parse_complete(write_half).await;
    Ok(())
}

struct BindRequest {
    portal: String,
    statement: String,
    parameter_formats: Vec<i16>,
    parameters: Vec<Option<Vec<u8>>>,
    result_formats: Vec<i16>,
}

async fn handle_bind(
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    bind: BindRequest,
) -> Result<(), ExtendedQueryError> {
    let BindRequest {
        portal,
        statement,
        parameter_formats,
        parameters,
        result_formats,
    } = bind;
    validate_format_codes(&parameter_formats, "parameter")?;
    validate_format_codes(&result_formats, "result")?;
    let prepared = state
        .prepared_statements
        .get(&statement)
        .cloned()
        .ok_or_else(|| missing_statement_error(&statement))?;
    if parameters.len() != prepared.parameter_count {
        return Err(ExtendedQueryError::protocol(format!(
            "bind for statement '{}' requires {} parameters but got {}",
            prepared.name,
            prepared.parameter_count,
            parameters.len()
        )));
    }
    validate_format_count(parameter_formats.len(), parameters.len(), "parameter")?;

    let values = parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            decode_parameter(
                parameter.as_deref(),
                format_for_index(&parameter_formats, index),
                prepared.parameter_types[index],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let replaced_portal = state.portals.contains_key(&portal);
    state.portals.insert(
        portal.clone(),
        Portal {
            name: portal,
            statement_name: statement,
            prepared_id: prepared.id,
            params: values,
            result_formats,
            described: prepared.described,
            suspended: None,
        },
    );
    if !replaced_portal {
        runtime.record_pgwire_portal_delta(1);
    }

    let _ = write_bind_complete(write_half).await;
    Ok(())
}

async fn handle_describe(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    target: DescribeTarget,
    name: String,
) -> Result<(), ExtendedQueryError> {
    match target {
        DescribeTarget::Statement => {
            let prepared = state
                .prepared_statements
                .get(&name)
                .cloned()
                .ok_or_else(|| missing_statement_error(&name))?;
            let columns = describe_prepared(cassie, prepared.clone()).await?;
            let row_frame = row_description_or_no_data_frame(&columns, &[])?;
            write_parameter_description(write_half, &prepared.parameter_types)
                .await
                .map_err(|error| ExtendedQueryError::write_failed(&error))?;
            write_frame(write_half, &row_frame).await?;
            if let Some(statement) = state.prepared_statements.get_mut(&name) {
                statement.described = true;
            }
        }
        DescribeTarget::Portal => {
            let portal = state
                .portals
                .get(&name)
                .cloned()
                .ok_or_else(|| missing_portal_error(&name))?;
            let prepared = prepared_for_portal(state, &portal)?;
            let columns = describe_prepared(cassie, prepared).await?;
            let row_frame = row_description_or_no_data_frame(&columns, &portal.result_formats)?;
            write_frame(write_half, &row_frame).await?;
            if let Some(portal) = state.portals.get_mut(&name) {
                portal.described = true;
            }
        }
    }

    Ok(())
}

async fn handle_execute(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    portal_name: String,
    max_rows: i32,
) -> Result<(), ExtendedQueryError> {
    runtime.record_pgwire_extended_query();
    let max_rows = usize::try_from(max_rows)
        .map_err(|_| ExtendedQueryError::protocol("invalid execute row limit"))?;
    let portal = state
        .portals
        .get(&portal_name)
        .cloned()
        .ok_or_else(|| missing_portal_error(&portal_name))?;

    if let Some(suspended) = portal.suspended.clone() {
        write_suspended_result(
            write_half,
            state,
            &portal_name,
            suspended,
            &portal.result_formats,
            max_rows,
        )
        .await?;
        return Ok(());
    }

    let prepared = prepared_for_portal(state, &portal)?;
    let session = session.clone();
    let params = portal.params.clone();
    let parsed = prepared.parsed.clone();
    let sql_fingerprint = prepared.sql_fingerprint;
    let result = run_pgwire_blocking(cassie, "pgwire_extended_query", move |cassie| {
        cassie.execute_parsed_sql_with_mode(
            &session,
            parsed,
            sql_fingerprint,
            params,
            ExecutionMode::ExtendedQuery,
        )
    })
    .await
    .map_err(|error| ExtendedQueryError::cassie(&error))?;

    write_fresh_result(
        write_half,
        state,
        &portal_name,
        &portal,
        &prepared,
        result,
        max_rows,
    )
    .await
}

async fn handle_close(
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    target: DescribeTarget,
    name: String,
) -> Result<(), ExtendedQueryError> {
    match target {
        DescribeTarget::Statement => {
            let prepared = state
                .prepared_statements
                .remove(&name)
                .ok_or_else(|| missing_statement_error(&name))?;
            runtime.record_pgwire_prepared_delta(-1);
            remove_portals_for_prepared_id(state, runtime, prepared.id);
        }
        DescribeTarget::Portal => {
            state
                .portals
                .remove(&name)
                .ok_or_else(|| missing_portal_error(&name))?;
            runtime.record_pgwire_portal_delta(-1);
        }
    }
    let _ = write_close_complete(write_half).await;
    Ok(())
}

async fn describe_prepared(
    cassie: Arc<Cassie>,
    prepared: PreparedStatement,
) -> Result<Vec<ColumnMeta>, ExtendedQueryError> {
    run_pgwire_blocking(cassie, "pgwire_describe", move |cassie| {
        cassie.describe_parsed_statement_with_parameter_oids(
            prepared.parsed,
            prepared.sql_fingerprint,
            &prepared.parameter_types,
        )
    })
    .await
    .map_err(|error| ExtendedQueryError::cassie(&error))
}

fn prepared_for_portal(
    state: &SessionState,
    portal: &Portal,
) -> Result<PreparedStatement, ExtendedQueryError> {
    let prepared = state
        .prepared_statements
        .get(&portal.statement_name)
        .cloned()
        .ok_or_else(|| missing_statement_error(&portal.statement_name))?;
    if prepared.id != portal.prepared_id {
        return Err(missing_portal_error(&portal.name));
    }
    Ok(prepared)
}

async fn write_fresh_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    portal_name: &str,
    portal: &Portal,
    prepared: &PreparedStatement,
    result: QueryResult,
    max_rows: usize,
) -> Result<(), ExtendedQueryError> {
    let QueryResult {
        columns,
        rows,
        command,
    } = result;
    let row_description_already_sent = portal.described || prepared.described;
    let row_description_sent = write_result_frames(
        write_half,
        &columns,
        &rows,
        &command,
        &portal.result_formats,
        row_description_already_sent,
        max_rows,
    )
    .await?;
    update_portal_after_execute(
        state,
        portal_name,
        columns,
        rows,
        command,
        row_description_sent,
        max_rows,
    );
    Ok(())
}

async fn write_suspended_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    portal_name: &str,
    suspended: PortalSuspended,
    result_formats: &[i16],
    max_rows: usize,
) -> Result<(), ExtendedQueryError> {
    let PortalSuspended {
        columns,
        rows,
        command,
        next_row,
        row_description_sent,
    } = suspended;
    let end_row = execution_end_row(next_row, rows.len(), max_rows);
    let remains_suspended = end_row < rows.len();
    let mut frames = Vec::new();
    let mut row_description_sent = row_description_sent;

    if !row_description_sent && !columns.is_empty() {
        append_row_description_frame(&mut frames, &columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
        row_description_sent = true;
    }
    for row in rows[next_row..end_row].iter().cloned() {
        append_data_row_frame(&mut frames, row, &columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    }
    if remains_suspended {
        append_portal_suspended_frame(&mut frames)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    } else {
        append_command_complete_frame(&mut frames, &command)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    }
    write_frame(write_half, &frames).await?;

    if let Some(portal) = state.portals.get_mut(portal_name) {
        portal.described |= row_description_sent;
        portal.suspended = remains_suspended.then_some(PortalSuspended {
            columns,
            rows,
            command,
            next_row: end_row,
            row_description_sent,
        });
    }
    Ok(())
}

async fn write_result_frames(
    write_half: &mut (impl AsyncWrite + Unpin),
    columns: &[ColumnMeta],
    rows: &[Vec<Value>],
    command: &str,
    result_formats: &[i16],
    row_description_already_sent: bool,
    max_rows: usize,
) -> Result<bool, ExtendedQueryError> {
    let end_row = execution_end_row(0, rows.len(), max_rows);
    let remains_suspended = end_row < rows.len();
    let mut frames = Vec::new();
    let mut row_description_sent = row_description_already_sent;

    if !row_description_sent && !columns.is_empty() {
        append_row_description_frame(&mut frames, columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
        row_description_sent = true;
    }
    for row in rows[..end_row].iter().cloned() {
        append_data_row_frame(&mut frames, row, columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    }
    if remains_suspended {
        append_portal_suspended_frame(&mut frames)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    } else {
        append_command_complete_frame(&mut frames, command)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    }
    write_frame(write_half, &frames).await?;
    Ok(row_description_sent)
}

fn update_portal_after_execute(
    state: &mut SessionState,
    portal_name: &str,
    columns: Vec<ColumnMeta>,
    rows: Vec<Vec<Value>>,
    command: String,
    row_description_sent: bool,
    max_rows: usize,
) {
    let end_row = execution_end_row(0, rows.len(), max_rows);
    let remains_suspended = end_row < rows.len();
    if let Some(portal) = state.portals.get_mut(portal_name) {
        portal.described |= row_description_sent;
        portal.suspended = remains_suspended.then_some(PortalSuspended {
            columns,
            rows,
            command,
            next_row: end_row,
            row_description_sent,
        });
    }
}

fn execution_end_row(start_row: usize, row_count: usize, max_rows: usize) -> usize {
    if max_rows == 0 {
        return row_count;
    }
    start_row.saturating_add(max_rows).min(row_count)
}

fn row_description_or_no_data_frame(
    columns: &[ColumnMeta],
    result_formats: &[i16],
) -> Result<Vec<u8>, ExtendedQueryError> {
    let mut frame = Vec::new();
    if columns.is_empty() {
        return Ok(frame);
    }
    append_row_description_frame(&mut frame, columns, result_formats)
        .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    Ok(frame)
}

async fn write_frame(
    write_half: &mut (impl AsyncWrite + Unpin),
    frame: &[u8],
) -> Result<(), ExtendedQueryError> {
    if frame.is_empty() {
        write_no_data(write_half)
            .await
            .map_err(|error| ExtendedQueryError::write_failed(&error))?;
        return Ok(());
    }
    write_half
        .write_all(frame)
        .await
        .map_err(|error| ExtendedQueryError::write_failed(&error))?;
    write_half
        .flush()
        .await
        .map_err(|error| ExtendedQueryError::write_failed(&error))
}

fn remove_portals_for_prepared_id(
    state: &mut SessionState,
    runtime: &RuntimeState,
    prepared_id: u64,
) {
    let before = state.portals.len();
    state
        .portals
        .retain(|_, portal| portal.prepared_id != prepared_id);
    let removed = before.saturating_sub(state.portals.len());
    if removed > 0 {
        let delta = isize::try_from(removed).unwrap_or(isize::MAX);
        runtime.record_pgwire_portal_delta(-delta);
    }
}

fn normalize_parameter_type_oids(parameter_type_oids: &[i32]) -> Vec<i32> {
    parameter_type_oids
        .iter()
        .map(|oid| if *oid == 0 { OID_UNKNOWN } else { *oid })
        .collect()
}

fn validate_format_codes(formats: &[i16], label: &str) -> Result<(), ExtendedQueryError> {
    if formats.iter().all(|format| matches!(*format, 0 | 1)) {
        return Ok(());
    }
    Err(ExtendedQueryError::protocol(format!(
        "unsupported {label} format code"
    )))
}

fn validate_format_count(
    format_count: usize,
    parameter_count: usize,
    label: &str,
) -> Result<(), ExtendedQueryError> {
    if format_count == 0 || format_count == 1 || format_count == parameter_count {
        return Ok(());
    }
    Err(ExtendedQueryError::protocol(format!(
        "unsupported {label} format count"
    )))
}

fn format_for_index(formats: &[i16], index: usize) -> i16 {
    match formats.len() {
        0 => 0,
        1 => formats[0],
        _ => formats[index],
    }
}

fn decode_parameter(
    parameter: Option<&[u8]>,
    format: i16,
    oid: i32,
) -> Result<Value, ExtendedQueryError> {
    let Some(parameter) = parameter else {
        return Ok(Value::Null);
    };
    if format == 0 {
        return decode_text_parameter(parameter, oid);
    }
    decode_binary_parameter(parameter, oid)
}

fn decode_text_parameter(parameter: &[u8], oid: i32) -> Result<Value, ExtendedQueryError> {
    let text = str::from_utf8(parameter)
        .map_err(|_| ExtendedQueryError::protocol("bind parameter is not valid UTF-8"))?;
    match oid {
        OID_BOOL => parse_bool(text).map(Value::Bool),
        OID_INT2 | OID_INT4 | OID_INT8 => text
            .parse::<i64>()
            .map(Value::Int64)
            .map_err(|_| ExtendedQueryError::protocol("invalid integer bind parameter")),
        OID_FLOAT8 => text
            .parse::<f64>()
            .map(Value::Float64)
            .map_err(|_| ExtendedQueryError::protocol("invalid float bind parameter")),
        OID_JSON => serde_json::from_str(text)
            .map(Value::Json)
            .map_err(|_| ExtendedQueryError::protocol("invalid JSON bind parameter")),
        _ => Ok(Value::String(text.to_string())),
    }
}

fn decode_binary_parameter(parameter: &[u8], oid: i32) -> Result<Value, ExtendedQueryError> {
    match oid {
        OID_BOOL => fixed_bytes::<1>(parameter, "bool").map(|bytes| Value::Bool(bytes[0] != 0)),
        OID_INT2 => fixed_bytes::<2>(parameter, "int2")
            .map(i16::from_be_bytes)
            .map(|value| Value::Int64(i64::from(value))),
        OID_INT4 => fixed_bytes::<4>(parameter, "int4")
            .map(i32::from_be_bytes)
            .map(|value| Value::Int64(i64::from(value))),
        OID_INT8 => fixed_bytes::<8>(parameter, "int8")
            .map(i64::from_be_bytes)
            .map(Value::Int64),
        OID_FLOAT8 => fixed_bytes::<8>(parameter, "float8")
            .map(f64::from_be_bytes)
            .map(Value::Float64),
        OID_BYTEA => Ok(Value::String(hex_bytea(parameter))),
        OID_JSON => {
            let text = str::from_utf8(parameter)
                .map_err(|_| ExtendedQueryError::protocol("JSON bind parameter is not UTF-8"))?;
            serde_json::from_str(text)
                .map(Value::Json)
                .map_err(|_| ExtendedQueryError::protocol("invalid JSON bind parameter"))
        }
        OID_UUID if parameter.len() == 16 => uuid::Uuid::from_slice(parameter)
            .map(|value| Value::String(value.to_string()))
            .map_err(|_| ExtendedQueryError::protocol("invalid UUID bind parameter")),
        OID_TEXT | OID_BPCHAR | OID_VARCHAR | OID_UNKNOWN | OID_UUID => str::from_utf8(parameter)
            .map(|text| Value::String(text.to_string()))
            .map_err(|_| ExtendedQueryError::protocol("bind parameter is not valid UTF-8")),
        _ => str::from_utf8(parameter)
            .map(|text| Value::String(text.to_string()))
            .map_err(|_| ExtendedQueryError::protocol("bind parameter is not valid UTF-8")),
    }
}

fn fixed_bytes<const N: usize>(
    parameter: &[u8],
    type_name: &str,
) -> Result<[u8; N], ExtendedQueryError> {
    parameter.try_into().map_err(|_| {
        ExtendedQueryError::protocol(format!("invalid binary {type_name} bind parameter"))
    })
}

fn parse_bool(text: &str) -> Result<bool, ExtendedQueryError> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "t" | "1" => Ok(true),
        "false" | "f" | "0" => Ok(false),
        _ => Err(ExtendedQueryError::protocol(
            "invalid boolean bind parameter",
        )),
    }
}

fn hex_bytea(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(2 + bytes.len().saturating_mul(2));
    out.push_str("\\x");
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn missing_statement_error(name: &str) -> ExtendedQueryError {
    ExtendedQueryError::invalid_name(format!("prepared statement '{name}' does not exist"))
}

fn missing_portal_error(name: &str) -> ExtendedQueryError {
    ExtendedQueryError::invalid_name(format!("portal '{name}' is not bound"))
}

#[derive(Debug)]
struct ExtendedQueryError {
    pg_error: Box<PgWireError>,
    record_protocol_error: bool,
}

impl ExtendedQueryError {
    fn protocol(message: impl Into<String>) -> Self {
        Self {
            pg_error: Box::new(PgWireError::protocol(message)),
            record_protocol_error: true,
        }
    }

    fn invalid_name(message: impl Into<String>) -> Self {
        Self {
            pg_error: Box::new(PgWireError::invalid_sql_statement_name(message)),
            record_protocol_error: true,
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        let error = CassieError::Unsupported(message.into());
        Self::cassie(&error)
    }

    fn cassie(error: &CassieError) -> Self {
        Self {
            pg_error: Box::new(PgWireError::from_cassie_error(PgWireSeverity::Error, error)),
            record_protocol_error: false,
        }
    }

    fn protocol_from_io(error: &io::Error) -> Self {
        Self::protocol(error.to_string())
    }

    fn write_failed(error: &io::Error) -> Self {
        Self {
            pg_error: Box::new(PgWireError::protocol(format!(
                "failed to write pgwire response: {error}"
            ))),
            record_protocol_error: true,
        }
    }
}

impl From<CassieError> for ExtendedQueryError {
    fn from(error: CassieError) -> Self {
        Self::cassie(&error)
    }
}
