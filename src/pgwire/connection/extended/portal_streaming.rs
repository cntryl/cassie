use std::sync::Arc;

use tokio::io::AsyncWrite;

use super::portal_state::{
    write_streaming_result, write_suspended_result, PortalExecution, PortalFetchWindow,
    StreamingPortalResult, SuspendedPortalWriteRequest,
};
use super::{
    describe_prepared, run_pgwire_blocking, Cassie, CassieError, CassieSession, ExecutionMode,
    ExtendedQueryError, PortalSuspended, PreparedStatement, QueryResult, SessionState,
};
use crate::app::PortalReadSpec;
use crate::midge::adapter::RowDecode;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{QueryStatement, SelectItem};
use crate::types::Value;

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
    if let Some(cancellation) = request.suspended.cancellation.as_ref() {
        if cancellation.is_cancelled() {
            request.state.clear_query_cancellation(cancellation);
            request.state.clear_portal_execution(request.portal_name);
            return Err(ExtendedQueryError::cassie(&CassieError::QueryCancelled));
        }
    }
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
    let result_cap = cassie.runtime.limits().max_result_rows;
    write_suspended_result(
        write_half,
        SuspendedPortalWriteRequest {
            state: &mut *request.state,
            portal_name: request.portal_name,
            portal: request.portal,
            suspended: request.suspended,
            max_rows: request.max_rows,
            result_cap,
        },
    )
    .await
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
    let resolved =
        cassie.resolve_portal_read_spec(request.session, request.prepared.parsed.clone());
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            clear_request_execution(&mut request);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    if let Some(mut spec) = resolved {
        if request.session.has_collection_changes(&spec.collection) {
            request.state.portal_cursors.remove(request.portal_name);
            return execute_offset_portal_page(cassie, write_half, request).await;
        }
        if spec.includes_wildcard {
            let Some(schema) = cassie.catalog.get_schema(&spec.collection) else {
                request.state.portal_cursors.remove(request.portal_name);
                return execute_offset_portal_page(cassie, write_half, request).await;
            };
            for field in schema.fields.iter().map(|field| field.name.clone()) {
                if !spec.source_fields.contains(&field) {
                    spec.source_fields.push(field);
                }
            }
        }
        let cursor = request
            .state
            .portal_cursors
            .remove(request.portal_name)
            .map_or_else(
                || {
                    cassie.midge.open_row_cursor(
                        &spec.collection,
                        RowDecode::ProjectedHistorical(spec.source_fields.clone()),
                    )
                },
                |cursor| Ok(Some(cursor)),
            );
        let cursor = match cursor {
            Ok(cursor) => cursor,
            Err(error) => {
                clear_request_execution(&mut request);
                return Err(ExtendedQueryError::cassie(&error));
            }
        };
        if let Some(cursor) = cursor {
            return execute_cursor_portal_page(cassie, write_half, request, spec, cursor).await;
        }
    }
    execute_offset_portal_page(cassie, write_half, request).await
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
    let mut parsed = request.prepared.parsed.clone();
    let crate::sql::ast::QueryStatement::Select(select) = &mut parsed.statement else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol(
            "streaming portal requires a select statement",
        ));
    };
    let result_cap = cassie.runtime.limits().max_result_rows;
    let window = PortalFetchWindow::new(result_cap, request.rows_emitted, request.max_rows);
    let fetch_rows = window.page_rows().saturating_add(1);
    select.limit = Some(i64::try_from(fetch_rows).unwrap_or(i64::MAX));
    select.offset = Some(i64::try_from(request.rows_emitted).unwrap_or(i64::MAX));
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
    let params = request.portal.params.clone();
    let result = run_pgwire_blocking(cassie, "pgwire_extended_query", move |cassie| {
        cassie.execute_parsed_sql_with_cancellation(
            &session,
            parsed,
            fingerprint,
            params,
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
    let remains_suspended = result.rows.len() > window.page_rows();
    let suspended_cancellation = if remains_suspended {
        Some(cancellation.suspend())
    } else {
        drop(cancellation);
        None
    };
    let rows_emitted = request.rows_emitted;
    write_streaming_result(
        write_half,
        &mut *request.state,
        request.portal_name,
        request.portal,
        StreamingPortalResult {
            result,
            cancellation: suspended_cancellation,
            has_more: remains_suspended,
            rows_emitted,
            window,
        },
    )
    .await
    .map(|_| ())
}

async fn execute_cursor_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    mut request: StreamingPortalPageRequest<'_>,
    spec: PortalReadSpec,
    mut cursor: crate::midge::adapter::MidgeRowCursor,
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
        &cassie.runtime.limits(),
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
        let page = cursor.next_page(&cassie.midge, requested_rows, &controls)?;
        let reservation = controls.reserve_query_memory(portal_page_bytes(&page.0)?)?;
        let peak_bytes = controls.peak_query_memory_bytes();
        Ok((cursor, page, reservation, peak_bytes))
    })
    .await;
    let (cursor, (documents, remains_suspended), _page_memory, peak_bytes) = match fetched {
        Ok(fetched) => fetched,
        Err(error) => {
            request.state.clear_portal_execution(request.portal_name);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    cassie.runtime.record_query_peak_memory(peak_bytes);
    let rows = portal_document_rows(&cassie, &spec, request.prepared, documents);
    let command = format!("SELECT {}", request.rows_emitted.saturating_add(rows.len()));
    let suspended_cancellation = if remains_suspended {
        Some(cancellation.suspend())
    } else {
        drop(cancellation);
        None
    };
    let portal_name = request.portal_name.to_string();
    let remains_suspended = write_streaming_result(
        write_half,
        &mut *request.state,
        &portal_name,
        request.portal,
        StreamingPortalResult {
            result: QueryResult {
                columns,
                rows,
                command,
            },
            cancellation: suspended_cancellation,
            has_more: remains_suspended,
            rows_emitted: request.rows_emitted,
            window,
        },
    )
    .await?;
    if remains_suspended {
        request.state.portal_cursors.insert(portal_name, cursor);
    }
    Ok(())
}

fn portal_page_bytes(
    documents: &[crate::midge::adapter::DocumentRef],
) -> Result<usize, CassieError> {
    documents.iter().try_fold(0usize, |total, document| {
        let payload_bytes = serde_json::to_vec(&document.payload)
            .map_err(|error| CassieError::Parse(error.to_string()))?
            .len();
        Ok(total
            .saturating_add(document.id.len())
            .saturating_add(payload_bytes))
    })
}

fn portal_document_rows(
    cassie: &Cassie,
    spec: &PortalReadSpec,
    prepared: &PreparedStatement,
    documents: Vec<crate::midge::adapter::DocumentRef>,
) -> Vec<Vec<Value>> {
    let schema = cassie.catalog.get_schema(&spec.collection);
    let QueryStatement::Select(select) = &prepared.parsed.statement else {
        return Vec::new();
    };
    documents
        .into_iter()
        .map(|document| {
            let row = crate::executor::scan::projected_document_to_row(
                document,
                &spec.source_fields,
                schema.as_ref(),
            );
            select
                .projection
                .iter()
                .flat_map(|item| match item {
                    SelectItem::Wildcard => row
                        .entries()
                        .iter()
                        .map(|(_, value)| value.clone())
                        .collect::<Vec<_>>(),
                    SelectItem::Column { name, .. }
                    | SelectItem::Expr {
                        expr: crate::sql::ast::Expr::Column(name),
                        ..
                    } => vec![row.get(name).cloned().unwrap_or(Value::Null)],
                    _ => Vec::new(),
                })
                .collect()
        })
        .collect()
}

pub(super) fn streamable_portal_query(prepared: &PreparedStatement, max_rows: usize) -> bool {
    let crate::sql::ast::QueryStatement::Select(select) = &prepared.parsed.statement else {
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
