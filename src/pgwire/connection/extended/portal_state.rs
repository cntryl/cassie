use tokio::io::AsyncWrite;

use super::super::writers::{
    append_command_complete_frame, append_data_row_frame, append_portal_suspended_frame,
    append_row_description_frame,
};
use super::{
    write_frame, CassieError, ExtendedQueryError, Portal, PortalSuspended, PreparedStatement,
    QueryResult, SessionState, Value,
};

#[derive(Debug, Clone)]
pub(super) struct PortalExecution {
    pub(super) name: String,
    pub(super) statement_name: String,
    pub(super) prepared_id: u64,
    pub(super) params: Vec<Value>,
    pub(super) result_formats: Vec<i16>,
    pub(super) described: bool,
}

impl From<&Portal> for PortalExecution {
    fn from(portal: &Portal) -> Self {
        Self {
            name: portal.name.clone(),
            statement_name: portal.statement_name.clone(),
            prepared_id: portal.prepared_id,
            params: portal.params.clone(),
            result_formats: portal.result_formats.clone(),
            described: portal.described,
        }
    }
}

pub(super) fn take_portal_execution(
    state: &mut SessionState,
    portal_name: &str,
) -> Option<(PortalExecution, Option<PortalSuspended>)> {
    let portal = state.portals.get_mut(portal_name)?;
    let suspended = portal.suspended.take();
    Some((PortalExecution::from(&*portal), suspended))
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PortalFetchWindow {
    page_rows: usize,
    reaches_result_cap: bool,
    result_cap: usize,
}

impl PortalFetchWindow {
    pub(super) fn new(result_cap: usize, rows_emitted: usize, max_rows: usize) -> Self {
        let remaining = result_cap.saturating_sub(rows_emitted);
        let page_rows = if max_rows == 0 {
            remaining
        } else {
            max_rows.min(remaining)
        };
        Self {
            page_rows,
            reaches_result_cap: page_rows == remaining,
            result_cap,
        }
    }

    pub(super) const fn page_rows(self) -> usize {
        self.page_rows
    }

    pub(super) const fn rejects_lookahead(self, has_more: bool) -> bool {
        self.reaches_result_cap && has_more
    }

    fn overflow_error(self) -> ExtendedQueryError {
        ExtendedQueryError::cassie(&CassieError::ResourceLimit(format!(
            "query result row limit exceeded: more than {} rows",
            self.result_cap
        )))
    }
}

pub(super) struct FreshPortalWriteRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a PortalExecution,
    pub(super) prepared: &'a PreparedStatement,
    pub(super) result: QueryResult,
    pub(super) cancellation: Option<crate::runtime::QueryCancellationHandle>,
    pub(super) max_rows: usize,
    pub(super) result_cap: usize,
}

pub(super) async fn write_fresh_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    request: FreshPortalWriteRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    let FreshPortalWriteRequest {
        state,
        portal_name,
        portal,
        prepared,
        result,
        cancellation,
        max_rows,
        result_cap,
    } = request;
    let QueryResult {
        columns,
        mut rows,
        command,
    } = result;
    let window = PortalFetchWindow::new(result_cap, 0, max_rows);
    if window.rejects_lookahead(rows.len() > window.page_rows()) {
        clear_cancellation(state, cancellation.as_ref());
        return Err(window.overflow_error());
    }

    let page_len = window.page_rows().min(rows.len());
    let portal_memory = if page_len < rows.len() {
        Some(
            reserve_portal_memory(state, &columns, &rows[page_len..], &command, false)
                .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))?,
        )
    } else {
        None
    };
    let remaining = rows.split_off(page_len);
    let remains_suspended = !remaining.is_empty();
    let row_description_sent = portal.described || prepared.described;
    let row_description_sent = write_portal_page_frames(
        write_half,
        PortalPageFrames {
            columns: &columns,
            rows,
            command: &command,
            result_formats: &portal.result_formats,
            row_description_already_sent: row_description_sent,
            remains_suspended,
        },
    )
    .await
    .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))?;

    let suspended = remains_suspended.then_some(PortalSuspended {
        columns,
        rows: remaining,
        command,
        row_description_sent,
        streaming: false,
        rows_emitted: page_len,
        cancellation,
        portal_memory,
    });
    store_portal_state(state, portal_name, row_description_sent, suspended);
    Ok(())
}

pub(super) struct SuspendedPortalWriteRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a PortalExecution,
    pub(super) suspended: PortalSuspended,
    pub(super) max_rows: usize,
    pub(super) result_cap: usize,
}

pub(super) async fn write_suspended_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    request: SuspendedPortalWriteRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    let SuspendedPortalWriteRequest {
        state,
        portal_name,
        portal,
        mut suspended,
        max_rows,
        result_cap,
    } = request;
    let window = PortalFetchWindow::new(result_cap, suspended.rows_emitted, max_rows);
    if window.rejects_lookahead(suspended.rows.len() > window.page_rows()) {
        clear_cancellation(state, suspended.cancellation.as_ref());
        return Err(window.overflow_error());
    }

    let page_len = window.page_rows().min(suspended.rows.len());
    let remaining = suspended.rows.split_off(page_len);
    let rows = std::mem::take(&mut suspended.rows);
    let remains_suspended = !remaining.is_empty();
    let row_description_sent = write_portal_page_frames(
        write_half,
        PortalPageFrames {
            columns: &suspended.columns,
            rows,
            command: &suspended.command,
            result_formats: &portal.result_formats,
            row_description_already_sent: suspended.row_description_sent,
            remains_suspended,
        },
    )
    .await
    .inspect_err(|_| clear_cancellation(state, suspended.cancellation.as_ref()))?;

    suspended.rows = remaining;
    suspended.row_description_sent = row_description_sent;
    suspended.rows_emitted = suspended.rows_emitted.saturating_add(page_len);
    if let Some(portal_memory) = suspended.portal_memory.as_mut() {
        portal_memory.shrink_to(portal_state_bytes(
            &suspended.columns,
            &suspended.rows,
            &suspended.command,
            false,
        ));
    }
    let cancellation = suspended.cancellation.clone();
    let suspended = remains_suspended.then_some(suspended);
    store_portal_state(state, portal_name, row_description_sent, suspended);
    if !remains_suspended {
        clear_cancellation(state, cancellation.as_ref());
    }
    Ok(())
}

pub(super) struct StreamingPortalResult {
    pub(super) result: QueryResult,
    pub(super) cancellation: Option<crate::runtime::QueryCancellationHandle>,
    pub(super) has_more: bool,
    pub(super) rows_emitted: usize,
    pub(super) window: PortalFetchWindow,
}

pub(super) async fn write_streaming_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    portal_name: &str,
    portal: &PortalExecution,
    streaming: StreamingPortalResult,
) -> Result<bool, ExtendedQueryError> {
    let StreamingPortalResult {
        result,
        cancellation,
        has_more,
        rows_emitted,
        window,
    } = streaming;
    if window.rejects_lookahead(has_more) {
        clear_cancellation(state, cancellation.as_ref());
        return Err(window.overflow_error());
    }

    let QueryResult {
        columns,
        mut rows,
        command,
    } = result;
    rows.truncate(window.page_rows());
    let page_len = rows.len();
    let portal_memory = if has_more {
        Some(
            reserve_portal_memory(state, &columns, &[], &command, true)
                .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))?,
        )
    } else {
        None
    };
    let row_description_sent = write_portal_page_frames(
        write_half,
        PortalPageFrames {
            columns: &columns,
            rows,
            command: &command,
            result_formats: &portal.result_formats,
            row_description_already_sent: portal.described,
            remains_suspended: has_more,
        },
    )
    .await
    .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))?;

    let suspended = has_more.then_some(PortalSuspended {
        columns,
        rows: Vec::new(),
        command,
        row_description_sent,
        streaming: true,
        rows_emitted: rows_emitted.saturating_add(page_len),
        cancellation,
        portal_memory,
    });
    store_portal_state(state, portal_name, row_description_sent, suspended);
    Ok(has_more)
}

struct PortalPageFrames<'a> {
    columns: &'a [crate::executor::ColumnMeta],
    rows: Vec<Vec<Value>>,
    command: &'a str,
    result_formats: &'a [i16],
    row_description_already_sent: bool,
    remains_suspended: bool,
}

async fn write_portal_page_frames(
    write_half: &mut (impl AsyncWrite + Unpin),
    page: PortalPageFrames<'_>,
) -> Result<bool, ExtendedQueryError> {
    let PortalPageFrames {
        columns,
        rows,
        command,
        result_formats,
        row_description_already_sent,
        remains_suspended,
    } = page;
    let mut frames = Vec::new();
    let mut row_description_sent = row_description_already_sent;
    if !row_description_sent && !columns.is_empty() {
        append_row_description_frame(&mut frames, columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
        row_description_sent = true;
    }
    for row in rows {
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

fn store_portal_state(
    state: &mut SessionState,
    portal_name: &str,
    row_description_sent: bool,
    suspended: Option<PortalSuspended>,
) {
    if let Some(portal) = state.portals.get_mut(portal_name) {
        portal.described |= row_description_sent;
        portal.suspended = suspended;
    } else if let Some(cancellation) = suspended
        .as_ref()
        .and_then(|suspended| suspended.cancellation.as_ref())
    {
        state.clear_query_cancellation(cancellation);
    }
}

fn clear_cancellation(
    state: &SessionState,
    cancellation: Option<&crate::runtime::QueryCancellationHandle>,
) {
    if let Some(cancellation) = cancellation {
        state.clear_query_cancellation(cancellation);
    }
}

fn reserve_portal_memory(
    state: &SessionState,
    columns: &[crate::executor::ColumnMeta],
    rows: &[Vec<Value>],
    command: &str,
    streaming: bool,
) -> Result<crate::runtime::QueryMemoryReservation, ExtendedQueryError> {
    state
        .portal_memory_controls
        .reserve_query_memory(portal_state_bytes(columns, rows, command, streaming))
        .map_err(|error| ExtendedQueryError::cassie(&error))
}

fn portal_state_bytes(
    columns: &[crate::executor::ColumnMeta],
    rows: &[Vec<Value>],
    command: &str,
    streaming: bool,
) -> usize {
    let columns_bytes = columns
        .iter()
        .fold(std::mem::size_of_val(columns), |bytes, column| {
            bytes
                .saturating_add(column.name.len())
                .saturating_add(column.data_type.len())
        });
    let rows_bytes = rows.iter().fold(std::mem::size_of_val(rows), |bytes, row| {
        row.iter().fold(
            bytes.saturating_add(std::mem::size_of_val(row.as_slice())),
            |bytes, value| bytes.saturating_add(value_variable_bytes(value)),
        )
    });
    let cursor_bytes = if streaming {
        std::mem::size_of::<crate::midge::adapter::MidgeRowCursor>()
    } else {
        0
    };
    columns_bytes
        .saturating_add(rows_bytes)
        .saturating_add(command.len())
        .saturating_add(cursor_bytes)
}

fn value_variable_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) => value.len(),
        Value::Vector(value) => value
            .values
            .len()
            .saturating_mul(std::mem::size_of::<f32>()),
        Value::Json(value) => json_variable_bytes(value),
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 0,
    }
}

fn json_variable_bytes(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(value) => value.len(),
        serde_json::Value::Array(values) => values.iter().fold(
            values
                .len()
                .saturating_mul(std::mem::size_of::<serde_json::Value>()),
            |bytes, value| bytes.saturating_add(json_variable_bytes(value)),
        ),
        serde_json::Value::Object(values) => values.iter().fold(0usize, |bytes, (key, value)| {
            bytes
                .saturating_add(key.len())
                .saturating_add(std::mem::size_of::<serde_json::Value>())
                .saturating_add(json_variable_bytes(value))
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => 0,
    }
}
