use std::sync::Arc;

use tokio::io::AsyncWrite;

use super::portal_state::{
    write_materialized_result, write_streaming_result, write_suspended_result,
    MaterializedPortalWriteRequest, PortalExecution, PortalFetchWindow, StreamingPortalResult,
    SuspendedPortalWriteRequest,
};
use super::{
    describe_prepared, run_pgwire_blocking, Cassie, CassieSession, ExecutionMode,
    ExtendedQueryError, PortalSuspended, PreparedStatement, SessionState,
};
use crate::app::transport::portal::{PortalCursorDecision, PortalReadPage};
use crate::runtime::QueryExecutionControls;

pub(super) struct SuspendedPortalRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) session: &'a CassieSession,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a PortalExecution,
    pub(super) suspended: PortalSuspended,
    pub(super) max_rows: usize,
}

pub(super) async fn resume_suspended_portal(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    request: SuspendedPortalRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    if request.suspended.streaming {
        let prepared = match super::prepared_for_portal(request.state, request.portal) {
            Ok(prepared) => prepared,
            Err(error) => {
                if let Some(cancellation) = request.suspended.cancellation.as_ref() {
                    request.state.clear_query_cancellation(cancellation);
                }
                request.state.clear_portal_execution(request.portal_name);
                return Err(error);
            }
        };
        let rows_emitted = request.suspended.rows_emitted;
        let cancellation = request.suspended.cancellation;
        return execute_streaming_portal_page(
            cassie,
            write_half,
            StreamingPortalPageRequest {
                state: request.state,
                session: request.session,
                portal_name: request.portal_name,
                portal: request.portal,
                prepared: &prepared,
                max_rows: request.max_rows,
                rows_emitted,
                cancellation,
            },
        )
        .await;
    }
    if let Err(error) = cassie.ensure_session_database_access(request.session) {
        if let Some(cancellation) = request.suspended.cancellation.as_ref() {
            request.state.clear_query_cancellation(cancellation);
        }
        request.state.clear_portal_execution(request.portal_name);
        return Err(ExtendedQueryError::cassie(&error));
    }
    let cancellation = {
        let registration = request
            .state
            .backend_registration
            .as_ref()
            .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
        let previous = request.suspended.cancellation.clone().unwrap_or_default();
        registration.resume_query(previous)
    };
    let active_cancellation = cancellation.handle();
    let result_cap = cassie.runtime().limits().max_result_rows;
    let state = &mut *request.state;
    let portal_name = request.portal_name;
    let result = write_suspended_result(
        write_half,
        SuspendedPortalWriteRequest {
            state,
            portal_name,
            portal: request.portal,
            suspended: request.suspended,
            active_cancellation: Some(active_cancellation),
            max_rows: request.max_rows,
            result_cap,
        },
    )
    .await;
    if result.is_err() {
        state.clear_portal_execution(portal_name);
    }
    result
}

pub(super) struct StreamingPortalPageRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) session: &'a CassieSession,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a PortalExecution,
    pub(super) prepared: &'a PreparedStatement,
    pub(super) max_rows: usize,
    pub(super) rows_emitted: usize,
    pub(super) cancellation: Option<crate::runtime::QueryCancellationHandle>,
}

pub(super) async fn execute_streaming_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    mut request: StreamingPortalPageRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    let Some(parsed) = request.prepared.parsed.clone() else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol(
            "streaming portal requires a select statement",
        ));
    };
    if let Err(error) = prepare_transaction_view(&cassie, &request, &parsed).await {
        clear_request_execution(&mut request);
        return Err(error);
    }
    request.session.mark_transaction_query();
    let plan = crate::app::transport::portal::resolve_portal_read_plan(
        &cassie,
        request.session,
        parsed.clone(),
    );
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            clear_request_execution(&mut request);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    if !plan.has_direct_cursor_plan() {
        return execute_offset_portal_page(cassie, write_half, request).await;
    }
    // Resolve first, as a resumed cursor must not skip plan errors or fresh
    // projection metadata; only then remove the cursor retained by transport.
    let suspended_cursor = request.state.portal_cursors.remove(request.portal_name);
    let page = crate::app::transport::portal::open_portal_page(
        &cassie,
        request.session,
        plan,
        suspended_cursor,
    );
    match page {
        Ok(PortalCursorDecision::Cursor(page)) => {
            return execute_cursor_portal_page(cassie, write_half, request, page, parsed).await;
        }
        Ok(PortalCursorDecision::Fallback(retained)) => {
            if let Some(retained) = retained {
                request
                    .state
                    .portal_cursors
                    .insert(request.portal_name.to_owned(), retained);
            }
        }
        Err(error) => {
            clear_request_execution(&mut request);
            return Err(ExtendedQueryError::cassie(&error));
        }
    }
    execute_offset_portal_page(cassie, write_half, request).await
}

async fn prepare_transaction_view(
    cassie: &Arc<Cassie>,
    request: &StreamingPortalPageRequest<'_>,
    parsed: &crate::sql::ParsedStatement,
) -> Result<(), ExtendedQueryError> {
    if request
        .session
        .repeatable_read_capture_generation()
        .is_none()
    {
        return Ok(());
    }
    let registration = request
        .state
        .backend_registration
        .as_ref()
        .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
    let cancellation = registration.begin_query();
    let handle = cancellation.handle();
    let session = request.session.clone();
    session.note_operation_worker_started();
    let parsed = parsed.clone();
    run_pgwire_blocking(
        Arc::clone(cassie),
        "pgwire_portal_snapshot",
        move |cassie| {
            let started = std::time::Instant::now();
            let mut controls = QueryExecutionControls::with_cancellation(
                &cassie.runtime().limits(),
                started,
                handle,
            );
            session.apply_statement_timeout(&mut controls, started);
            let result = cassie.prepare_transaction_read_view(&session, &parsed, &controls);
            cassie.runtime().record_query_memory(&controls);
            result
        },
    )
    .await
    .map_err(|error| ExtendedQueryError::cassie(&error))
}

fn clear_request_execution(request: &mut StreamingPortalPageRequest<'_>) {
    if let Some(cancellation) = request.cancellation.as_ref() {
        request.state.clear_query_cancellation(cancellation);
    }
    request.state.clear_portal_execution(request.portal_name);
}

async fn execute_offset_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    mut request: StreamingPortalPageRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    let Some(mut parsed) = request.prepared.parsed.clone() else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol(
            "streaming portal requires a select statement",
        ));
    };
    let crate::sql::ast::QueryStatement::Select(select) = &mut parsed.statement else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol(
            "streaming portal requires a select statement",
        ));
    };
    let result_cap = cassie.runtime().limits().max_result_rows;
    // A real cursor snapshot isn't available here, so the underlying source can
    // shift under concurrent writes. Fetch everything up to the result cap in
    // one shot and cache the remainder (see `write_materialized_result`)
    // instead of re-querying with OFFSET on every subsequent page, which would
    // silently skip or repeat rows when the source mutates between pages.
    select.limit = Some(crate::sql::ast::Expr::IntegerLiteral(
        i64::try_from(result_cap.saturating_add(1)).unwrap_or(i64::MAX),
    ));
    select.offset = Some(crate::sql::ast::Expr::IntegerLiteral(
        i64::try_from(request.rows_emitted).unwrap_or(i64::MAX),
    ));
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let Some(registration) = request.state.backend_registration.as_ref() else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol("backend is not registered"));
    };
    let cancellation = request.cancellation.clone().map_or_else(
        || registration.begin_query(),
        |handle| registration.resume_query(handle),
    );
    let cancellation_handle = cancellation.handle();
    let session = request.session.clone();
    let sql = request.prepared.query.clone();
    let params = request.portal.params.clone();
    let parameter_type_oids = request.prepared.parameter_types.clone();
    let result = run_pgwire_blocking(cassie, "pgwire_extended_query", move |cassie| {
        cassie.execute_parsed_sql_with_cancellation(
            &session,
            (parsed, &sql),
            fingerprint,
            (params, &parameter_type_oids),
            ExecutionMode::ExtendedQuery,
            &cancellation_handle,
        )
    })
    .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            request.state.clear_portal_execution(request.portal_name);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    let active_cancellation = Some(cancellation.handle());
    let rows_emitted = request.rows_emitted;
    let row_description_sent = request.portal.described || request.prepared.described;
    write_materialized_result(
        write_half,
        MaterializedPortalWriteRequest {
            state: &mut *request.state,
            portal_name: request.portal_name,
            portal: request.portal,
            result,
            cancellation: active_cancellation,
            max_rows: request.max_rows,
            result_cap,
            rows_emitted,
            row_description_sent,
        },
    )
    .await?;
    super::super::writers::write_changed_parameter_statuses(
        write_half,
        request.session,
        &mut request.state.parameter_statuses,
    )
    .await
    .map_err(|error| ExtendedQueryError::write_failed(&error))?;
    Ok(())
}

async fn execute_cursor_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    mut request: StreamingPortalPageRequest<'_>,
    page: PortalReadPage,
    parsed: crate::sql::ParsedStatement,
) -> Result<(), ExtendedQueryError> {
    let columns = describe_prepared(
        Arc::clone(&cassie),
        request.session.clone(),
        request.prepared.clone(),
    )
    .await;
    let columns = match columns {
        Ok(columns) => columns,
        Err(error) => {
            clear_request_execution(&mut request);
            return Err(error);
        }
    };
    let Some(registration) = request.state.backend_registration.as_ref() else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol("backend is not registered"));
    };
    let cancellation = request.cancellation.clone().map_or_else(
        || registration.begin_query(),
        |handle| registration.resume_query(handle),
    );
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime().limits(),
        std::time::Instant::now(),
        cancellation.handle(),
    );
    let window = PortalFetchWindow::new(
        controls.max_result_rows,
        request.rows_emitted,
        request.max_rows,
    );
    let requested_rows = window.page_rows();
    let fetched = run_pgwire_blocking(Arc::clone(&cassie), "pgwire_portal_page", move |cassie| {
        crate::app::transport::portal::fetch_portal_page(
            &cassie,
            page,
            parsed,
            &columns,
            requested_rows,
            &controls,
        )
    })
    .await;
    let (cursor, result, remains_suspended, peak_bytes) = match fetched {
        Ok(fetched) => fetched,
        Err(error) => {
            request.state.clear_portal_execution(request.portal_name);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    cassie.runtime().record_query_peak_memory(peak_bytes);
    let active_cancellation = Some(cancellation.handle());
    let portal_name = request.portal_name.to_string();
    let remains_suspended = write_streaming_result(
        write_half,
        &mut *request.state,
        &portal_name,
        request.portal,
        StreamingPortalResult {
            result,
            cancellation: active_cancellation,
            has_more: remains_suspended,
            rows_emitted: request.rows_emitted,
            window,
            row_description_sent: request.portal.described || request.prepared.described,
        },
    )
    .await?;
    if remains_suspended {
        request.state.portal_cursors.insert(portal_name, cursor);
    }
    Ok(())
}

pub(super) fn streamable_portal_query(prepared: &PreparedStatement, max_rows: usize) -> bool {
    let Some(crate::sql::ast::QueryStatement::Select(select)) =
        prepared.parsed.as_ref().map(|parsed| &parsed.statement)
    else {
        return false;
    };
    max_rows > 0
        && select.ctes.is_empty()
        && !select.distinct
        && select.distinct_on.is_empty()
        && select.filter.is_none()
        && select.group_by.is_empty()
        && select.having.is_none()
        && select.order.is_empty()
        && select.limit.is_none()
        && select.offset.is_none()
        && select.set.is_none()
        && matches!(select.source, crate::sql::ast::QuerySource::Collection(_))
        && select.projection.iter().all(|item| {
            matches!(
                item,
                crate::sql::ast::SelectItem::Wildcard
                    | crate::sql::ast::SelectItem::Column { .. }
                    | crate::sql::ast::SelectItem::Expr {
                        expr: crate::sql::ast::Expr::Column(_),
                        ..
                    }
            )
        })
}
