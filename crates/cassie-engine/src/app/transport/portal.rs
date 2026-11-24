//! Retained document-cursor operations for streaming portal transports.

use crate::app::{Cassie, CassieError, CassieSession, PortalReadSpec};
use crate::executor::batch::BatchRow;
use crate::executor::{ColumnMeta, QueryResult};
use crate::midge::adapter::{MidgeRowCursor, RowDecode, StatementReadScope};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{ParsedStatement, QueryStatement, SelectItem, SelectStatement};
use crate::types::row_identity::{is_identity_reference, ROW_IDENTITY_COLUMN};
use crate::types::Value;

/// Opaque cursor ownership retained by a suspended wire portal.
///
/// The cursor pins its Midge read transaction and its admitted snapshot memory.
/// Keeping the handle across Execute messages prevents later session writes
/// from replacing the snapshot used by the first page.
#[derive(Debug)]
pub struct RetainedPortal {
    cursor: MidgeRowCursor,
}

/// One current read plan paired with the cursor that will serve this page.
pub struct PortalReadPage {
    spec: PortalReadSpec,
    cursor: MidgeRowCursor,
}

/// The freshly resolved semantic plan for one portal Execute.
pub struct PortalReadPlan {
    spec: Option<PortalReadSpec>,
}

impl PortalReadPlan {
    /// Reports whether the current plan can be considered for direct cursor use.
    #[must_use]
    pub const fn has_direct_cursor_plan(&self) -> bool {
        self.spec.is_some()
    }
}

/// Result of the engine's cursor-plan decision for a transport Execute.
pub enum PortalCursorDecision {
    /// A direct cursor is available for this page.
    Cursor(PortalReadPage),
    /// Use the transport's materialized fallback, retaining the old handle only
    /// when plan resolution returned no direct cursor plan.
    Fallback(Option<RetainedPortal>),
}

/// Resolves the current read plan before the transport takes a suspended cursor.
///
/// # Errors
/// Returns query planning errors.
pub fn resolve_portal_read_plan(
    cassie: &Cassie,
    session: &CassieSession,
    parsed: ParsedStatement,
) -> Result<PortalReadPlan, CassieError> {
    let spec = cassie.resolve_portal_read_spec(session, parsed)?;
    Ok(PortalReadPlan { spec })
}

/// Opens a cursor from a resolved plan or reuses the exact suspended cursor.
///
/// A retained cursor bypasses staged-write and virtual-view fallbacks while
/// continuing its pinned snapshot. Fresh plan resolution still occurs before
/// the caller removes the cursor from transport state.
///
/// # Errors
/// Returns snapshot, storage, or resource errors.
pub fn open_portal_page(
    cassie: &Cassie,
    session: &CassieSession,
    plan: PortalReadPlan,
    retained: Option<RetainedPortal>,
) -> Result<PortalCursorDecision, CassieError> {
    let Some(mut spec) = plan.spec else {
        return Ok(PortalCursorDecision::Fallback(retained));
    };
    let owner = session.repeatable_committed_read(
        &cassie.midge,
        session
            .current_database()
            .unwrap_or(&cassie.default_database),
    )?;
    // Preserve the repeatable-read check and temporary scope before deciding
    // whether this page reuses a cursor or takes a materialized fallback.
    let _read_scope = StatementReadScope::enter(owner.as_ref());
    if retained.is_none()
        && (crate::catalog::virtual_views::schema(&spec.collection).is_some()
        || cassie.catalog.get_view(&spec.collection).is_some()
        || session.has_collection_changes(&spec.collection))
    {
        return Ok(PortalCursorDecision::Fallback(None));
    }
    if spec.includes_wildcard {
        let Some(schema) = cassie.catalog.get_schema(&spec.collection) else {
            return Ok(PortalCursorDecision::Fallback(None));
        };
        for field in &schema.fields {
            if !spec.source_fields.iter().any(|source| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(source, &field.name)
            }) {
                spec.source_fields.push(field.name.clone());
            }
        }
    }

    // The retained cursor itself continues its original read snapshot.
    let cursor = if let Some(retained) = retained {
        retained.cursor
    } else {
        let Some(cursor) = cassie.midge.open_row_cursor_with_traversal(
            &spec.collection,
            RowDecode::ProjectedHistorical(spec.source_fields.clone()),
            spec.traversal,
        )? else {
            return Ok(PortalCursorDecision::Fallback(None));
        };
        cursor
    };
    Ok(PortalCursorDecision::Cursor(PortalReadPage { spec, cursor }))
}

/// Fetches and projects one admitted page while preserving the cursor owner.
///
/// `requested_rows` must already be capped by the transport's result window.
/// The returned handle is the only way to continue this exact snapshot.
///
/// # Errors
/// Returns storage, projection, cancellation, deadline, or memory-admission errors.
pub fn fetch_portal_page(
    cassie: &Cassie,
    mut page: PortalReadPage,
    parsed: ParsedStatement,
    columns: &[ColumnMeta],
    requested_rows: usize,
    controls: &QueryExecutionControls,
) -> Result<(RetainedPortal, QueryResult, bool, usize), CassieError> {
    let documents = page.cursor.next_accounted_page(
        &cassie.midge,
        requested_rows,
        controls,
    )?;
    let has_more = documents.has_more();
    let QueryStatement::Select(select) = &parsed.statement else {
        return Err(CassieError::Execution(
            "streaming portal requires a select statement".to_owned(),
        ));
    };
    let result = portal_document_result(
        cassie,
        &page.spec,
        select,
        columns,
        documents,
        controls,
    )?;
    let peak_bytes = controls.peak_query_memory_bytes();
    Ok((RetainedPortal { cursor: page.cursor }, result, has_more, peak_bytes))
}

fn portal_document_result(
    cassie: &Cassie,
    spec: &PortalReadSpec,
    select: &SelectStatement,
    columns: &[ColumnMeta],
    mut documents: crate::midge::adapter::AccountedDocumentPage,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, CassieError> {
    let schema = cassie.catalog.get_schema(&spec.collection);
    let mut builder = QueryResult::try_builder(controls)?;
    for column in columns {
        builder.try_push_column(column)?;
    }
    builder.try_set_command("SELECT")?;
    // Identifier parsing and lookup builders allocate bounded scratch during
    // both the sizing pass and conversion, so admit it before either runs.
    let _identifiers = controls.reserve_query_memory(portal_identifier_bytes(
        spec,
        select,
        schema.as_ref(),
    )?)?;
    while let Some(document) = documents.pop_document() {
        controls.check_interruption()?;
        let document = document.document();
        let scratch = crate::executor::retained_memory::add(
            crate::executor::scan::projected_row_bytes(
                document,
                &spec.source_fields,
                schema.as_ref(),
            )?,
            crate::executor::scan::projected_lookup_bytes(
                &spec.source_fields,
                schema.as_ref(),
            )?,
        )?;
        let _projection = controls.reserve_query_memory(scratch)?;
        let row = crate::executor::scan::projected_document_to_row(
            document,
            &spec.source_fields,
            schema.as_ref(),
        );
        let row = crate::executor::scan::hydrate_character_arrays(row, controls)?;
        builder.try_push_values(portal_projection_values(select, &row, schema.as_ref()))?;
    }
    builder.finish()
}

fn portal_identifier_bytes(
    spec: &PortalReadSpec,
    select: &SelectStatement,
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
    select: &'a SelectStatement,
    row: &'a BatchRow,
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
