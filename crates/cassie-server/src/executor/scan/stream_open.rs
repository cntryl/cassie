use super::{
    controlled_storage_error, uses_controlled_row_scan, Cassie, CassieSession, ProjectedScanStream,
    QueryExecutionControls, RowDecode,
};
use crate::midge::adapter::RowTraversal;

#[cfg(test)]
pub(crate) fn projected_scan_stream<'a>(
    cassie: &'a Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
    limit: Option<usize>,
    controls: &'a QueryExecutionControls,
) -> Result<Option<ProjectedScanStream<'a>>, crate::executor::QueryError> {
    projected_scan_stream_with_traversal(
        cassie,
        session,
        collection,
        fields,
        limit,
        RowTraversal::CompleteSource,
        controls,
    )
}

pub(crate) fn projected_scan_stream_with_traversal<'a>(
    cassie: &'a Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
    limit: Option<usize>,
    traversal: RowTraversal,
    controls: &'a QueryExecutionControls,
) -> Result<Option<ProjectedScanStream<'a>>, crate::executor::QueryError> {
    if !uses_controlled_row_scan(cassie, collection) {
        return Ok(None);
    }
    let Some(cursor) = cassie
        .open_session_row_cursor_with_traversal(
            session,
            collection,
            RowDecode::ProjectedHistorical(fields.to_vec()),
            traversal,
            controls,
        )
        .map_err(|error| controlled_storage_error(cassie, error))?
    else {
        return Ok(None);
    };
    cassie.runtime.record_parallel_scan_fallback();
    Ok(Some(ProjectedScanStream {
        cassie,
        cursor,
        fields: fields.to_vec(),
        schema: cassie.catalog.get_schema(collection),
        controls,
        remaining: limit.unwrap_or(usize::MAX),
        previous_page_memory: None,
    }))
}
