//! The streamed exact scan over heap-table rows whose vector is decoded straight into `f32`.
//!
//! The generic cursor turns every vector element into a JSON number before ranking. This path
//! reads the packed components instead, so a page is charged four bytes per component rather
//! than the decoded-JSON footprint. Ranking, ties, NULL handling and diagnostics are the same
//! [`Ranker`] the generic loop uses.

use super::exact::{filter_document, ExactVectorRequest, FilterContext, FilterOutcome, Ranker};
use super::QueryError;
use crate::app::Cassie;
use crate::midge::adapter::{DocumentRef, MidgeRowCursor};
use crate::midge::row_blob::VectorColumn;

const DIRECT_PAGE_BYTE_TARGET: usize = 1 << 20;
const DIRECT_PAGE_ROWS_MAX: usize = 256;
/// Per-row bookkeeping beyond the vector buffer.
const DIRECT_ROW_OVERHEAD_BYTES: usize = 256;

/// Rows per storage page so the in-flight `f32` buffers stay near the page byte target.
pub(super) fn direct_page_rows(dimensions: usize) -> usize {
    let row_bytes = dimensions
        .saturating_mul(size_of::<f32>())
        .saturating_add(DIRECT_ROW_OVERHEAD_BYTES);
    (DIRECT_PAGE_BYTE_TARGET / row_bytes).clamp(1, DIRECT_PAGE_ROWS_MAX)
}

/// The cursor, vector column, ranker and scanned-row counter one direct scan advances.
pub(super) struct DirectScan<'a, 'r> {
    pub(super) cursor: &'a mut MidgeRowCursor,
    pub(super) column: &'a VectorColumn,
    pub(super) ranker: &'a mut Ranker<'r>,
    pub(super) scanned: &'a mut usize,
}

/// Ranks every row of the cursor. `false` means the generic executor must answer instead.
pub(super) fn rank_direct_pages(
    cassie: &Cassie,
    scan: &mut DirectScan<'_, '_>,
    (request, context): (&ExactVectorRequest<'_>, &FilterContext<'_>),
    page_rows: usize,
) -> Result<bool, QueryError> {
    let with_payload = request.filter_expr.is_some();
    loop {
        let rows = scan.cursor.next_vector_rows(
            &cassie.midge,
            page_rows,
            request.controls,
            scan.column,
            with_payload,
        )?;
        if rows.is_empty() {
            return Ok(true);
        }
        for row in rows {
            super::super::super::check_timeout(request.controls)?;
            *scan.scanned = scan.scanned.saturating_add(1);
            if let Some(payload) = row.payload {
                let mut document = DocumentRef {
                    id: row.id.clone(),
                    payload,
                };
                if matches!(
                    filter_document(&mut document, request, context)?,
                    FilterOutcome::Rejected
                ) {
                    continue;
                }
            }
            if !scan.ranker.rank(request.controls, &row.id, row.vector)? {
                return Ok(false);
            }
        }
    }
}
