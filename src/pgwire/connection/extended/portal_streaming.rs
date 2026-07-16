use std::sync::Arc;

use tokio::io::AsyncWrite;

use super::{
    append_command_complete_frame, append_data_row_frame, append_portal_suspended_frame,
    append_row_description_frame, describe_prepared, run_pgwire_blocking, write_frame,
    write_suspended_result, Cassie, CassieError, CassieSession, ExecutionMode, ExtendedQueryError,
    Portal, PortalSuspended, PreparedStatement, QueryResult, SessionState,
};
use crate::midge::adapter::RowDecode;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{QuerySource, QueryStatement, SelectItem};
use crate::types::Value;

pub(super) struct SuspendedPortalRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) session: &'a CassieSession,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a Portal,
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
            if let Some(registration) = request.state.backend_registration.as_ref() {
                registration.clear_query(cancellation);
            }
            if let Some(portal) = request.state.portals.get_mut(request.portal_name) {
                portal.suspended = None;
            }
            return Err(ExtendedQueryError::cassie(&CassieError::QueryCancelled));
        }
    }
    if let Some(query_offset) = request.suspended.query_offset {
        let prepared = super::prepared_for_portal(request.state, request.portal)?;
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
                query_offset,
            },
        )
        .await;
    }
    let cancellation = request.suspended.cancellation.clone();
    write_suspended_result(
        write_half,
        request.state,
        request.portal_name,
        request.suspended,
        &request.portal.result_formats,
        request.max_rows,
    )
    .await?;
    if request
        .state
        .portals
        .get(request.portal_name)
        .is_some_and(|portal| portal.suspended.is_none())
    {
        if let (Some(registration), Some(cancellation)) = (
            request.state.backend_registration.as_ref(),
            cancellation.as_ref(),
        ) {
            registration.clear_query(cancellation);
        }
    }
    Ok(())
}

pub(super) struct StreamingPortalPageRequest<'a> {
    pub(super) state: &'a mut SessionState,
    pub(super) session: &'a CassieSession,
    pub(super) portal_name: &'a str,
    pub(super) portal: &'a Portal,
    pub(super) prepared: &'a PreparedStatement,
    pub(super) max_rows: usize,
    pub(super) query_offset: usize,
}

pub(super) async fn execute_streaming_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    request: StreamingPortalPageRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    if let Some(mut spec) = portal_cursor_spec(request.prepared) {
        if spec.includes_wildcard {
            let Some(schema) = cassie.catalog.get_schema(&spec.collection) else {
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
            )?;
        if let Some(cursor) = cursor {
            return execute_cursor_portal_page(cassie, write_half, request, spec, cursor).await;
        }
    }
    execute_offset_portal_page(cassie, write_half, request).await
}

async fn execute_offset_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    request: StreamingPortalPageRequest<'_>,
) -> Result<(), ExtendedQueryError> {
    let mut parsed = request.prepared.parsed.clone();
    let crate::sql::ast::QueryStatement::Select(select) = &mut parsed.statement else {
        return Err(ExtendedQueryError::protocol(
            "streaming portal requires a select statement",
        ));
    };
    let fetch_rows = request.max_rows.saturating_add(1);
    select.limit = Some(i64::try_from(fetch_rows).unwrap_or(i64::MAX));
    select.offset = Some(i64::try_from(request.query_offset).unwrap_or(i64::MAX));
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let registration = request
        .state
        .backend_registration
        .as_ref()
        .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
    let cancellation = request
        .portal
        .suspended
        .as_ref()
        .and_then(|suspended| suspended.cancellation.clone())
        .map_or_else(
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
            if let Some(portal) = request.state.portals.get_mut(request.portal_name) {
                portal.suspended = None;
            }
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    let remains_suspended = result.rows.len() > request.max_rows;
    let suspended_cancellation = if remains_suspended {
        Some(cancellation.suspend())
    } else {
        drop(cancellation);
        None
    };
    write_streaming_portal_page(write_half, request, result, suspended_cancellation, None).await
}

struct PortalCursorSpec {
    collection: String,
    source_fields: Vec<String>,
    includes_wildcard: bool,
}

fn portal_cursor_spec(prepared: &PreparedStatement) -> Option<PortalCursorSpec> {
    let QueryStatement::Select(select) = &prepared.parsed.statement else {
        return None;
    };
    let QuerySource::Collection(collection) = &select.source else {
        return None;
    };
    let mut source_fields = Vec::new();
    let mut includes_wildcard = false;
    for item in &select.projection {
        match item {
            SelectItem::Wildcard => includes_wildcard = true,
            SelectItem::Column { name, .. }
            | SelectItem::Expr {
                expr: crate::sql::ast::Expr::Column(name),
                ..
            } => source_fields.push(name.clone()),
            _ => return None,
        }
    }
    Some(PortalCursorSpec {
        collection: collection.clone(),
        source_fields,
        includes_wildcard,
    })
}

async fn execute_cursor_portal_page(
    cassie: Arc<Cassie>,
    write_half: &mut (impl AsyncWrite + Unpin),
    request: StreamingPortalPageRequest<'_>,
    spec: PortalCursorSpec,
    mut cursor: crate::midge::adapter::MidgeRowCursor,
) -> Result<(), ExtendedQueryError> {
    let columns = describe_prepared(Arc::clone(&cassie), request.prepared.clone()).await?;
    let registration = request
        .state
        .backend_registration
        .as_ref()
        .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
    let cancellation = request
        .portal
        .suspended
        .as_ref()
        .and_then(|suspended| suspended.cancellation.clone())
        .map_or_else(
            || registration.begin_query(),
            |handle| registration.resume_query(handle),
        );
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        std::time::Instant::now(),
        cancellation.handle(),
    );
    let requested_rows = if request.max_rows == 0 {
        controls.max_result_rows
    } else {
        request.max_rows
    };
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
            if let Some(portal) = request.state.portals.get_mut(request.portal_name) {
                portal.suspended = None;
            }
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    cassie.runtime.record_query_peak_memory(peak_bytes);
    if request.max_rows == 0 && remains_suspended {
        if let Some(portal) = request.state.portals.get_mut(request.portal_name) {
            portal.suspended = None;
        }
        return Err(ExtendedQueryError::cassie(&CassieError::ResourceLimit(
            format!("query result row limit exceeded: more than {requested_rows} rows"),
        )));
    }
    let rows = portal_document_rows(&cassie, &spec, request.prepared, documents);
    let command = format!("SELECT {}", request.query_offset.saturating_add(rows.len()));
    let suspended_cancellation = if remains_suspended {
        request
            .state
            .portal_cursors
            .insert(request.portal_name.to_string(), cursor);
        Some(cancellation.suspend())
    } else {
        drop(cancellation);
        None
    };
    write_streaming_portal_page(
        write_half,
        request,
        QueryResult {
            columns,
            rows,
            command,
        },
        suspended_cancellation,
        Some(remains_suspended),
    )
    .await
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
    spec: &PortalCursorSpec,
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

async fn write_streaming_portal_page(
    write_half: &mut (impl AsyncWrite + Unpin),
    request: StreamingPortalPageRequest<'_>,
    result: QueryResult,
    cancellation: Option<crate::runtime::QueryCancellationHandle>,
    suspension_override: Option<bool>,
) -> Result<(), ExtendedQueryError> {
    let QueryResult {
        columns,
        mut rows,
        command,
    } = result;
    let remains_suspended = suspension_override.unwrap_or(rows.len() > request.max_rows);
    rows.truncate(request.max_rows);
    let mut frames = Vec::new();
    let mut row_description_sent = request.portal.described;
    if !row_description_sent && !columns.is_empty() {
        append_row_description_frame(&mut frames, &columns, &request.portal.result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
        row_description_sent = true;
    }
    for row in rows {
        append_data_row_frame(&mut frames, row, &columns, &request.portal.result_formats)
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

    if let Some(portal) = request.state.portals.get_mut(request.portal_name) {
        portal.described |= row_description_sent;
        portal.suspended = remains_suspended.then_some(PortalSuspended {
            columns,
            rows: Vec::new(),
            command,
            next_row: 0,
            row_description_sent,
            query_offset: Some(request.query_offset.saturating_add(request.max_rows)),
            cancellation,
        });
    }
    Ok(())
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
