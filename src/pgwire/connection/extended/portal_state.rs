use tokio::io::AsyncWrite;

use super::super::result_frames::QueryResultFrames;
use super::super::writers::ensure_row_widths;
use super::{
    write_frame, CassieError, ExtendedQueryError, Portal, PortalSuspended, PreparedStatement,
    QueryResult, SessionState, Value,
};

#[cfg(test)]
#[path = "portal_state/ownership_tests.rs"]
mod ownership_tests;

#[derive(Debug, Clone)]
pub(super) struct PortalExecution {
    pub(super) name: String,
    pub(super) statement_name: String,
    pub(super) prepared_id: u64,
    pub(super) params: Vec<Value>,
    pub(super) result_formats: Vec<i16>,
    pub(super) described: bool,
    pub(super) completed_command: Option<String>,
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
            completed_command: portal.completed_command.clone(),
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
    let row_description_sent = portal.described || prepared.described;
    write_materialized_result(
        write_half,
        MaterializedPortalWriteRequest {
            state,
            portal_name,
            portal,
            result,
            cancellation,
            max_rows,
            result_cap,
            rows_emitted: 0,
            row_description_sent,
        },
    )
    .await
}

pub(super) struct MaterializedPortalWriteRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a PortalExecution,
    pub(super) result: QueryResult,
    pub(super) cancellation: Option<crate::runtime::QueryCancellationHandle>,
    pub(super) max_rows: usize,
    pub(super) result_cap: usize,
    pub(super) rows_emitted: usize,
    pub(super) row_description_sent: bool,
}

/// Serves a portal page from a fully materialized result, caching the
/// remainder for subsequent resumes instead of re-executing the query. This
/// is what keeps pagination stable across concurrent writes: once the rows
/// are fetched here, later `Execute` calls only slice the cached vector.
pub(super) async fn write_materialized_result(
    write_half: &mut (impl AsyncWrite + Unpin),
    request: MaterializedPortalWriteRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    let MaterializedPortalWriteRequest {
        state,
        portal_name,
        portal,
        result,
        cancellation,
        max_rows,
        result_cap,
        rows_emitted,
        row_description_sent,
    } = request;
    let window = PortalFetchWindow::new(result_cap, rows_emitted, max_rows);
    if window.rejects_lookahead(result.row_count() > window.page_rows()) {
        clear_cancellation(state, cancellation.as_ref());
        return Err(window.overflow_error());
    }

    let page_len = window.page_rows().min(result.row_count());
    let remains_suspended = page_len < result.row_count();
    // A retained page admits the complete physical payload in the connection's
    // portal budget. Advancing an offset does not release still-owned capacity.
    let result = if remains_suspended {
        result
            .try_clone_for_query(&state.portal_memory_controls)
            .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))
            .map_err(|error| ExtendedQueryError::cassie(&error))?
    } else {
        result
    };
    let row_description_sent = write_portal_page_frames(
        write_half,
        PortalPageFrames {
            result: &result,
            rows: &result.rows()[..page_len],
            result_formats: &portal.result_formats,
            row_description_already_sent: row_description_sent,
            remains_suspended,
        },
    )
    .await
    .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))?;

    let completion = result.clone();
    let suspended = remains_suspended.then_some(PortalSuspended {
        result,
        row_offset: page_len,
        row_description_sent,
        streaming: false,
        rows_emitted: rows_emitted.saturating_add(page_len),
        cancellation,
        portal_memory: None,
    });
    store_portal_state(
        state,
        portal_name,
        row_description_sent,
        completion.command(),
        suspended,
    );
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
    let rows = suspended
        .result
        .rows()
        .get(suspended.row_offset..)
        .ok_or_else(|| ExtendedQueryError::protocol("invalid suspended portal row offset"))?;
    if window.rejects_lookahead(rows.len() > window.page_rows()) {
        clear_cancellation(state, suspended.cancellation.as_ref());
        return Err(window.overflow_error());
    }

    let page_len = window.page_rows().min(rows.len());
    let remains_suspended = page_len < rows.len();
    let row_description_sent = write_portal_page_frames(
        write_half,
        PortalPageFrames {
            result: &suspended.result,
            rows: &rows[..page_len],
            result_formats: &portal.result_formats,
            row_description_already_sent: suspended.row_description_sent,
            remains_suspended,
        },
    )
    .await
    .inspect_err(|_| clear_cancellation(state, suspended.cancellation.as_ref()))?;

    suspended.row_offset += page_len;
    suspended.row_description_sent = row_description_sent;
    suspended.rows_emitted = suspended.rows_emitted.saturating_add(page_len);
    let cancellation = suspended.cancellation.clone();
    let completion = suspended.result.clone();
    let suspended = remains_suspended.then_some(suspended);
    store_portal_state(
        state,
        portal_name,
        row_description_sent,
        completion.command(),
        suspended,
    );
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
    /// Whether a `RowDescription` already went out for this result, either
    /// through Describe(Portal) or a Describe(Statement) that landed after
    /// the portal was bound.
    pub(super) row_description_sent: bool,
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
        row_description_sent,
    } = streaming;
    if window.rejects_lookahead(has_more) {
        clear_cancellation(state, cancellation.as_ref());
        return Err(window.overflow_error());
    }

    let page_len = result.row_count().min(window.page_rows());
    let result = if has_more {
        result
            .try_clone_for_query(&state.portal_memory_controls)
            .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))
            .map_err(|error| ExtendedQueryError::cassie(&error))?
    } else {
        result
    };
    let portal_memory = if has_more {
        Some(
            state
                .portal_memory_controls
                .reserve_query_memory(std::mem::size_of::<crate::midge::adapter::MidgeRowCursor>())
                .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))
                .map_err(|error| ExtendedQueryError::cassie(&error))?,
        )
    } else {
        None
    };
    let row_description_sent = write_portal_page_frames(
        write_half,
        PortalPageFrames {
            result: &result,
            rows: &result.rows()[..page_len],
            result_formats: &portal.result_formats,
            row_description_already_sent: row_description_sent,
            remains_suspended: has_more,
        },
    )
    .await
    .inspect_err(|_| clear_cancellation(state, cancellation.as_ref()))?;

    let completion = result.clone();
    let suspended = has_more.then_some(PortalSuspended {
        row_offset: result.row_count(),
        result,
        row_description_sent,
        streaming: true,
        rows_emitted: rows_emitted.saturating_add(page_len),
        cancellation,
        portal_memory,
    });
    store_portal_state(
        state,
        portal_name,
        row_description_sent,
        completion.command(),
        suspended,
    );
    Ok(has_more)
}

struct PortalPageFrames<'a> {
    result: &'a QueryResult,
    rows: &'a [Vec<Value>],
    result_formats: &'a [i16],
    row_description_already_sent: bool,
    remains_suspended: bool,
}

async fn write_portal_page_frames(
    write_half: &mut (impl AsyncWrite + Unpin),
    page: PortalPageFrames<'_>,
) -> Result<bool, ExtendedQueryError> {
    let PortalPageFrames {
        result,
        rows,
        result_formats,
        row_description_already_sent,
        remains_suspended,
    } = page;
    let columns = result.columns();
    ensure_row_widths(rows, columns)
        .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    let mut frames = QueryResultFrames::try_new(result, rows, result_formats)
        .map_err(|error| ExtendedQueryError::cassie(&error))?;
    let mut row_description_sent = row_description_already_sent;
    if !row_description_sent && !columns.is_empty() {
        frames
            .description(columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
        write_frame(write_half, frames.bytes()).await?;
        row_description_sent = true;
    }
    for row in rows {
        frames
            .row(row, columns, result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
        write_frame(write_half, frames.bytes()).await?;
    }
    frames
        .completion(remains_suspended)
        .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    write_frame(write_half, frames.bytes()).await?;
    Ok(row_description_sent)
}

/// Writes the reply for an `Execute` against a portal that already ran to
/// completion: PostgreSQL reports an empty `CommandComplete` and never runs
/// the statement again.
pub(super) async fn write_completed_portal(
    write_half: &mut (impl AsyncWrite + Unpin),
    command: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), ExtendedQueryError> {
    let mut frames = QueryResultFrames::try_command(command, controls)
        .map_err(|error| ExtendedQueryError::cassie(&error))?;
    frames
        .completion(false)
        .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    write_frame(write_half, frames.bytes()).await
}

fn store_portal_state(
    state: &mut SessionState,
    portal_name: &str,
    row_description_sent: bool,
    command: &str,
    suspended: Option<PortalSuspended>,
) {
    if let Some(portal) = state.portals.get_mut(portal_name) {
        portal.described |= row_description_sent;
        if suspended.is_none() {
            portal.completed_command = Some(exhausted_command_tag(command));
        }
        portal.suspended = suspended;
    } else if let Some(cancellation) = suspended
        .as_ref()
        .and_then(|suspended| suspended.cancellation.as_ref())
    {
        state.clear_query_cancellation(cancellation);
    }
}

/// Rewrites a completed command tag to report zero affected rows, e.g.
/// `INSERT 0 1` becomes `INSERT 0 0` and `SELECT` becomes `SELECT 0`.
fn exhausted_command_tag(command: &str) -> String {
    match command.rsplit_once(' ') {
        Some((prefix, count)) if count.parse::<u64>().is_ok() => format!("{prefix} 0"),
        _ if command.eq_ignore_ascii_case("SELECT") => "SELECT 0".to_string(),
        _ => command.to_string(),
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
