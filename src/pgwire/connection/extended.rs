use std::io;
use std::str;
use std::sync::Arc;

use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::blocking::run_pgwire_blocking;
use super::codecs::{binary_to_value, validate_result_formats};
use super::errors::{PgWireError, PgWireSeverity};
use super::readers::read_frontend_message;
use super::state::{DescribeTarget, FrontendMessage, HandshakeError, HandshakeState, SessionState};
use super::writers::{
    append_row_description_frame, write_bind_complete, write_close_complete, write_error_response,
    write_no_data, write_parameter_description, write_parse_complete, write_ready_for_query,
};
use super::{ConnectionStep, PgwireReader};
use crate::app::{unsupported_sql_error, Cassie, CassieError, CassieSession};
use crate::executor::{ColumnMeta, QueryResult};
use crate::pgwire::protocol::{Portal, PortalSuspended, PreparedStatement};
use crate::runtime::{ExecutionMode, RuntimeState};
use crate::types::Value;

#[path = "extended/portal_state.rs"]
mod portal_state;
#[path = "extended/portal_streaming.rs"]
mod portal_streaming;
use portal_state::{
    take_portal_execution, write_fresh_result, FreshPortalWriteRequest, PortalExecution,
    PortalFetchWindow,
};
use portal_streaming::{
    execute_streaming_portal_page, resume_suspended_portal, streamable_portal_query,
    StreamingPortalPageRequest, SuspendedPortalRequest,
};

const OID_BOOL: i32 = 16;
const OID_INT8: i32 = 20;
const OID_INT2: i32 = 21;
const OID_INT4: i32 = 23;
const OID_JSON: i32 = 114;
const OID_FLOAT8: i32 = 701;
const OID_UNKNOWN: i32 = 705;

pub(super) async fn handle_frontend_message(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    reader: &mut PgwireReader,
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
            handle_bind(cassie, runtime, write_half, state, session, bind).await?;
        }
        FrontendMessage::Describe { target, name } => {
            runtime.record_pgwire_message("describe");
            handle_describe(cassie, write_half, state, session, target, name).await?;
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
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
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
    if !result_formats.is_empty() {
        let columns = describe_prepared(cassie, session.clone(), prepared.clone()).await?;
        validate_result_formats(&columns, &result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    }
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

    let replaced_portal = state.remove_portal(&portal).is_some();
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
    session: &CassieSession,
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
            let columns = describe_prepared(cassie, session.clone(), prepared.clone()).await?;
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
                .map(PortalExecution::from)
                .ok_or_else(|| missing_portal_error(&name))?;
            let prepared = prepared_for_portal(state, &portal)?;
            let columns = describe_prepared(cassie, session.clone(), prepared).await?;
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
    let (portal, suspended) = take_portal_execution(state, &portal_name)
        .ok_or_else(|| missing_portal_error(&portal_name))?;

    if let Some(suspended) = suspended {
        return resume_suspended_portal(
            cassie,
            write_half,
            SuspendedPortalRequest {
                state,
                session,
                portal_name: &portal_name,
                portal: &portal,
                suspended,
                max_rows,
            },
        )
        .await;
    }

    let prepared = prepared_for_portal(state, &portal)?;
    if streamable_portal_query(&prepared, max_rows) {
        return execute_streaming_portal_page(
            cassie,
            write_half,
            StreamingPortalPageRequest {
                state,
                session,
                portal_name: &portal_name,
                portal: &portal,
                prepared: &prepared,
                max_rows,
                rows_emitted: 0,
                cancellation: None,
            },
        )
        .await;
    }
    let session = session.clone();
    let params = portal.params.clone();
    let parsed = prepared.parsed.clone();
    let sql_fingerprint = prepared.sql_fingerprint;
    let registration = state
        .backend_registration
        .as_ref()
        .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
    let cancellation = registration.begin_query();
    let cancellation_handle = cancellation.handle();
    let result_cap = cassie.runtime.limits().max_result_rows;
    let result = run_pgwire_blocking(cassie, "pgwire_extended_query", move |cassie| {
        cassie.execute_parsed_sql_with_cancellation(
            &session,
            parsed,
            sql_fingerprint,
            params,
            ExecutionMode::ExtendedQuery,
            &cancellation_handle,
        )
    })
    .await
    .map_err(|error| ExtendedQueryError::cassie(&error))?;
    let window = PortalFetchWindow::new(result_cap, 0, max_rows);
    let suspended_cancellation = if window.page_rows() < result.rows.len() {
        Some(cancellation.suspend())
    } else {
        drop(cancellation);
        None
    };

    write_fresh_result(
        write_half,
        FreshPortalWriteRequest {
            state,
            portal_name: &portal_name,
            portal: &portal,
            prepared: &prepared,
            result,
            cancellation: suspended_cancellation,
            max_rows,
            result_cap,
        },
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
                .remove_portal(&name)
                .ok_or_else(|| missing_portal_error(&name))?;
            runtime.record_pgwire_portal_delta(-1);
        }
    }
    let _ = write_close_complete(write_half).await;
    Ok(())
}

async fn describe_prepared(
    cassie: Arc<Cassie>,
    session: CassieSession,
    prepared: PreparedStatement,
) -> Result<Vec<ColumnMeta>, ExtendedQueryError> {
    run_pgwire_blocking(cassie, "pgwire_describe", move |cassie| {
        cassie.describe_parsed_statement_for_session(
            &session,
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
    portal: &PortalExecution,
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
    let removed = state.remove_portals_for_prepared_id(prepared_id);
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

pub(super) fn benchmark_decode_parameter(
    parameter: &[u8],
    format: i16,
    oid: i32,
) -> Result<Value, String> {
    decode_parameter(Some(parameter), format, oid).map_err(|error| error.pg_error.message.clone())
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
    binary_to_value(parameter, i64::from(oid))
        .map_err(|error| ExtendedQueryError::protocol_from_io(&error))
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

fn missing_statement_error(name: &str) -> ExtendedQueryError {
    ExtendedQueryError::invalid_name(format!("prepared statement '{name}' does not exist"))
}

fn missing_portal_error(name: &str) -> ExtendedQueryError {
    ExtendedQueryError::invalid_name(format!("portal '{name}' is not bound"))
}

#[derive(Debug)]
pub(super) struct ExtendedQueryError {
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
        if error.kind() == io::ErrorKind::Unsupported {
            return Self::unsupported(error.to_string());
        }
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
