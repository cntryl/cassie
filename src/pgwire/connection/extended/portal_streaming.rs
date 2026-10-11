use std::sync::Arc;

use tokio::io::AsyncWrite;

use super::portal_state::{
    write_materialized_result, write_streaming_result, write_suspended_result,
    MaterializedPortalWriteRequest, PortalExecution, PortalFetchWindow, StreamingPortalResult,
    SuspendedPortalWriteRequest,
};
use super::{
    describe_prepared, run_pgwire_blocking, Cassie, CassieError, CassieSession, ExecutionMode,
    ExtendedQueryError, PortalSuspended, PreparedStatement, QueryResult, SessionState,
};
use crate::app::PortalReadSpec;
use crate::midge::adapter::RowDecode;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{QueryStatement, SelectItem};
use crate::types::row_identity::{is_identity_reference, ROW_IDENTITY_COLUMN};
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
    if let Err(error) = cassie.ensure_session_database_access(request.session) {
        if let Some(cancellation) = request.suspended.cancellation.as_ref() {
            request.state.clear_query_cancellation(cancellation);
        }
        request.state.clear_portal_execution(request.portal_name);
        return Err(ExtendedQueryError::cassie(&error));
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
    let Some(parsed) = request.prepared.parsed.clone() else {
        clear_request_execution(&mut request);
        return Err(ExtendedQueryError::protocol(
            "streaming portal requires a select statement",
        ));
    };
    let resolved = cassie.resolve_portal_read_spec(request.session, parsed);
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            clear_request_execution(&mut request);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    if let Some(mut spec) = resolved {
        // A suspended cursor keeps reading the snapshot it opened with, so it
        // must not be abandoned for an OFFSET re-query when this session later
        // stages writes: that would page over a different result.
        let suspended_cursor = request.state.portal_cursors.remove(request.portal_name);
        if suspended_cursor.is_none()
            && (crate::catalog::virtual_views::schema(&spec.collection).is_some()
                || cassie.catalog.get_view(&spec.collection).is_some())
        {
            return execute_offset_portal_page(cassie, write_half, request).await;
        }
        if suspended_cursor.is_none() && request.session.has_collection_changes(&spec.collection) {
            return execute_offset_portal_page(cassie, write_half, request).await;
        }
        if spec.includes_wildcard {
            let Some(schema) = cassie.catalog.get_schema(&spec.collection) else {
                return execute_offset_portal_page(cassie, write_half, request).await;
            };
            for field in &schema.fields {
                if !spec.source_fields.iter().any(|source| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(source, &field.name)
                }) {
                    spec.source_fields.push(field.name.clone());
                }
            }
        }
        let cursor = suspended_cursor.map_or_else(
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
    let result_cap = cassie.runtime.limits().max_result_rows;
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
    let params = request.portal.params.clone();
    let parameter_type_oids = request.prepared.parameter_types.clone();
    let result = run_pgwire_blocking(cassie, "pgwire_extended_query", move |cassie| {
        cassie.execute_parsed_sql_with_cancellation(
            &session,
            parsed,
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
    let window = PortalFetchWindow::new(result_cap, request.rows_emitted, request.max_rows);
    let remains_suspended = result.row_count() > window.page_rows();
    let suspended_cancellation = if remains_suspended {
        Some(cancellation.suspend())
    } else {
        drop(cancellation);
        None
    };
    let rows_emitted = request.rows_emitted;
    let row_description_sent = request.portal.described || request.prepared.described;
    write_materialized_result(
        write_half,
        MaterializedPortalWriteRequest {
            state: &mut *request.state,
            portal_name: request.portal_name,
            portal: request.portal,
            result,
            cancellation: suspended_cancellation,
            max_rows: request.max_rows,
            result_cap,
            rows_emitted,
            row_description_sent,
        },
    )
    .await
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
    let prepared = request.prepared.clone();
    let fetched = run_pgwire_blocking(Arc::clone(&cassie), "pgwire_portal_page", move |cassie| {
        let page = cursor.next_accounted_page(&cassie.midge, requested_rows, &controls)?;
        let remains_suspended = page.has_more();
        let result = portal_document_result(&cassie, &spec, &prepared, &columns, page, &controls)?;
        let peak_bytes = controls.peak_query_memory_bytes();
        Ok((cursor, result, remains_suspended, peak_bytes))
    })
    .await;
    let (cursor, result, remains_suspended, peak_bytes) = match fetched {
        Ok(fetched) => fetched,
        Err(error) => {
            request.state.clear_portal_execution(request.portal_name);
            return Err(ExtendedQueryError::cassie(&error));
        }
    };
    cassie.runtime.record_query_peak_memory(peak_bytes);
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
            result,
            cancellation: suspended_cancellation,
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

fn portal_document_result(
    cassie: &Cassie,
    spec: &PortalReadSpec,
    prepared: &PreparedStatement,
    columns: &[crate::executor::ColumnMeta],
    mut documents: crate::midge::adapter::AccountedDocumentPage,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, CassieError> {
    let schema = cassie.catalog.get_schema(&spec.collection);
    let Some(QueryStatement::Select(select)) =
        prepared.parsed.as_ref().map(|parsed| &parsed.statement)
    else {
        return Err(CassieError::Execution(
            "streaming portal requires a select statement".to_owned(),
        ));
    };
    let mut builder = QueryResult::try_builder(controls)?;
    for column in columns {
        builder.try_push_column(column)?;
    }
    builder.try_set_command("SELECT")?;
    // The existing conversion and lookup builders parse SQL identifiers. Admit
    // their bounded scratch before either the sizing pass or conversion runs.
    let _identifiers =
        controls.reserve_query_memory(portal_identifier_bytes(spec, select, schema.as_ref())?)?;
    while let Some(document) = documents.pop_document() {
        controls.check_interruption()?;
        let document = document.document();
        let scratch = crate::executor::retained_memory::add(
            crate::executor::scan::projected_row_bytes(
                document,
                &spec.source_fields,
                schema.as_ref(),
            )?,
            crate::executor::scan::projected_lookup_bytes(&spec.source_fields, schema.as_ref())?,
        )?;
        let _projection = controls.reserve_query_memory(scratch)?;
        let row = crate::executor::scan::projected_document_to_row(
            document,
            &spec.source_fields,
            schema.as_ref(),
        );
        builder.try_push_values(portal_projection_values(select, &row, schema.as_ref()))?;
    }
    builder.finish()
}

fn portal_identifier_bytes(
    spec: &PortalReadSpec,
    select: &crate::sql::ast::SelectStatement,
    schema: Option<&crate::catalog::CollectionSchema>,
) -> Result<usize, CassieError> {
    use crate::executor::retained_memory::{add, mul};

    let catalog_longest = schema
        .into_iter()
        .flat_map(|schema| &schema.fields)
        .map(|field| field.name.len())
        .max()
        .unwrap_or(0);
    let source_longest = spec
        .source_fields
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(0);
    let projection_longest = select
        .projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Column { name, .. }
            | SelectItem::Expr {
                expr: crate::sql::ast::Expr::Column(name),
                ..
            } => Some(name.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let longest = catalog_longest
        .max(source_longest)
        .max(projection_longest)
        .max(ROW_IDENTITY_COLUMN.len());
    add(512, add(mul(longest, 32)?, mul(catalog_longest, 8)?)?)
}

fn portal_projection_values<'a>(
    select: &'a crate::sql::ast::SelectStatement,
    row: &'a crate::executor::batch::BatchRow,
    schema: Option<&'a crate::catalog::CollectionSchema>,
) -> impl Iterator<Item = &'a Value> + Clone {
    let schema_has_id = schema.is_some_and(crate::catalog::CollectionSchema::declares_id);
    select.projection.iter().flat_map(move |item| {
        let wildcard = matches!(item, SelectItem::Wildcard);
        let column = match item {
            SelectItem::Column { name, .. }
            | SelectItem::Expr {
                expr: crate::sql::ast::Expr::Column(name),
                ..
            } => Some(if is_identity_reference(name, schema_has_id) {
                ROW_IDENTITY_COLUMN
            } else {
                name.as_str()
            }),
            _ => None,
        };
        // Match the RowDescription's declared schema order for wildcard fields.
        (wildcard && !schema_has_id)
            .then_some(ROW_IDENTITY_COLUMN)
            .into_iter()
            .chain(
                schema
                    .into_iter()
                    .flat_map(|schema| &schema.fields)
                    .filter(move |_| wildcard)
                    .map(|field| field.name.as_str()),
            )
            .chain(column)
            .map(move |name| row.get(name).unwrap_or(&Value::Null))
    })
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
