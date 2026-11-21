use std::collections::HashMap;
use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, BatchRow};
use crate::runtime::{QueryExecutionControls, QueryScoringTestBoundary};
use crate::sql::ast::Expr;
use crate::types::Value;

use super::candidate::{
    candidate_sort_value, compare_sql_vector_candidates, vector_rows_from_top, AccountedVectorTopK,
    SqlVectorCandidate,
};
use super::{
    adaptive_candidate_decision, record_adaptive_candidate_decision, vector_from_json,
    VectorDistanceTopKSpec,
};
use super::{filter, scan, value_to_vector, vector_prefilter_supported, QueryError};

pub(super) struct ExactVectorRequest<'a> {
    pub(super) session: Option<&'a CassieSession>,
    pub(super) stored_vector_field: &'a str,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) filter_expr: Option<&'a Expr>,
    pub(super) controls: &'a QueryExecutionControls,
}

pub(super) fn execute_exact_vector_top_k(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
    started_at: Instant,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let rows = run_exact_vector_top_k(cassie, spec, request, started_at)?;
    if rows.is_some() {
        super::diagnostics::record_exact_fallback_duration(
            cassie,
            spec,
            request.stored_vector_field,
            started_at,
        )?;
    }
    Ok(rows)
}

fn run_exact_vector_top_k(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
    started_at: Instant,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let top_needed = spec.top_needed();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    if let StreamOutcome::Selected(top, top_memory, counts) =
        stream_exact_vector_candidates(cassie, spec, request, StreamMode::Sql)?
    {
        let final_candidate_count = counts.scored;
        let rows = vector_rows_from_top(top, spec);
        drop(top_memory);
        cassie.runtime.record_vector_execution(
            started_at.elapsed(),
            final_candidate_count,
            rows.len(),
        );
        record_adaptive_candidate_decision(cassie, &adaptive, final_candidate_count, rows.len());
        return Ok(Some(rows));
    }

    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    let mut candidates = batch::flatten_batches(scan::scan(
        cassie,
        request.session,
        &spec.collection,
        request.controls,
    )?);
    if let Some(filter_expr) = request.filter_expr {
        if !vector_prefilter_supported(filter_expr, &schema) {
            return Ok(None);
        }
        let before = candidates.len();
        candidates = filter::filter_rows(
            candidates,
            filter_expr,
            request.params,
            None,
            request.user_functions,
            request.session,
        )?;
        cassie
            .runtime
            .record_vector_prefilter_usage(before, candidates.len(), None);
    }

    let mut final_candidate_count = 0usize;
    let mut top = AccountedVectorTopK::try_new(request.controls)?;
    for candidate in candidates {
        super::super::super::check_timeout(request.controls)?;
        let Some(value) = candidate
            .get(request.stored_vector_field)
            .filter(|value| !value.is_null())
        else {
            continue;
        };
        let vector = value_to_vector(value).unwrap_or_default();
        validate_exact_vector(spec, &vector)?;
        final_candidate_count = final_candidate_count.saturating_add(1);
        let score = spec.exact_distance(&vector);
        top.try_push(
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&spec.direction, score),
                score,
                id: candidate
                    .get(crate::types::row_identity::ROW_IDENTITY_COLUMN)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            top_needed,
        )?;
        request
            .controls
            .scored_candidate(QueryScoringTestBoundary::ExactVector)?;
    }
    super::super::super::check_timeout(request.controls)?;
    let (top, top_memory) = top.into_parts();
    let rows = vector_rows_from_top(top, spec);
    drop(top_memory);
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), final_candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, &adaptive, final_candidate_count, rows.len());
    Ok(Some(rows))
}

/// Rows requested per storage page.
///
/// A page of decoded rows is accounted at roughly `EXACT_DECODED_BYTES_PER_COMPONENT` bytes per
/// vector component until the rows are scored, so the page is sized to keep that in-flight
/// charge near `EXACT_PAGE_BYTE_TARGET` instead of scaling it with the 1,024-row executor batch.
pub(crate) fn exact_page_rows(dimensions: usize) -> usize {
    let row_bytes = dimensions
        .saturating_mul(EXACT_DECODED_BYTES_PER_COMPONENT)
        .max(1);
    (EXACT_PAGE_BYTE_TARGET / row_bytes).clamp(1, EXACT_PAGE_ROWS_MAX)
}

const EXACT_PAGE_BYTE_TARGET: usize = 1 << 20;
const EXACT_DECODED_BYTES_PER_COMPONENT: usize = 512;
const EXACT_PAGE_ROWS_MAX: usize = 256;

/// How the streamed exact scan treats rows it cannot score.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamMode {
    /// The vector-top-k SQL shape: the result is the whole answer, so an invalid vector is an
    /// error and rows without a vector are dropped.
    Sql,
    /// Candidate selection for a general query shape. Anything the generic executor would answer
    /// differently (an invalid vector, or an under-filled answer that would include NULL-vector
    /// rows) declines so the generic path produces its own result or diagnostic.
    Candidates,
}

pub(super) enum StreamOutcome {
    /// The cursor or the transaction overlay does not allow streaming.
    Unavailable,
    /// Streaming ran but the generic executor must answer.
    Declined,
    Selected(
        std::collections::BinaryHeap<SqlVectorCandidate>,
        crate::runtime::QueryMemoryReservation,
        ScanCounts,
    ),
}

/// Rows the streamed scan visited and the rows it scored.
#[derive(Clone, Copy)]
pub(super) struct ScanCounts {
    scanned: usize,
    scored: usize,
}

/// Ranked ids selected by a streamed exact scan, with the reservation that still covers them.
pub(crate) struct ExactCandidates {
    ranked: Vec<SqlVectorCandidate>,
    memory: crate::runtime::QueryMemoryReservation,
    scanned: usize,
}

impl ExactCandidates {
    /// Rows the scan visited before the filter, the figure the generic scan reports.
    pub(crate) const fn scanned_rows(&self) -> usize {
        self.scanned
    }

    /// Candidate ids in storage-key order, with the reservation covering them.
    pub(crate) fn into_sorted_ids(self) -> (Vec<String>, crate::runtime::QueryMemoryReservation) {
        let mut ids = self
            .ranked
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        (ids, self.memory)
    }
}

/// Selects the ranked exact top candidates for a general query shape without materializing rows.
pub(super) fn select_exact_candidates(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
) -> Result<Option<ExactCandidates>, QueryError> {
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, spec.top_needed())?;
    let StreamOutcome::Selected(top, memory, counts) =
        stream_exact_vector_candidates(cassie, spec, request, StreamMode::Candidates)?
    else {
        return Ok(None);
    };
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    // The generic plan metrics record the vector execution once the projected rows exist.
    record_adaptive_candidate_decision(cassie, &adaptive, counts.scored, ranked.len());
    Ok(Some(ExactCandidates {
        ranked,
        memory,
        scanned: counts.scanned,
    }))
}

fn stream_decode(request: &ExactVectorRequest<'_>) -> crate::midge::adapter::RowDecode {
    if request.filter_expr.is_some() {
        crate::midge::adapter::RowDecode::Full
    } else {
        crate::midge::adapter::RowDecode::ProjectedHistorical(vec![
            crate::sql::ColumnIdentifierPath::from_field_name(request.stored_vector_field)
                .canonical_sql(),
        ])
    }
}

fn stream_exact_vector_candidates(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
    mode: StreamMode,
) -> Result<StreamOutcome, QueryError> {
    if request
        .session
        .is_some_and(|session| session.has_collection_changes(&spec.collection))
    {
        return Ok(StreamOutcome::Unavailable);
    }
    let Some(mut cursor) = cassie
        .midge
        .open_row_cursor(&spec.collection, stream_decode(request))?
    else {
        return Ok(StreamOutcome::Unavailable);
    };
    let prefilter_supported = request.filter_expr.is_none_or(|filter_expr| {
        cassie
            .catalog
            .get_schema(&spec.collection)
            .is_some_and(|schema| vector_prefilter_supported(filter_expr, &schema))
    });
    if mode == StreamMode::Candidates && !prefilter_supported {
        return Ok(StreamOutcome::Declined);
    }
    let skipped_field = prefilter_supported.then_some(request.stored_vector_field);
    let schema = cassie.catalog.get_schema(&spec.collection);
    let mut ranker = Ranker::try_new(spec, mode, request.controls)?;
    let mut scanned_count = 0usize;
    let direct = if prefilter_supported {
        cursor.vector_column(request.stored_vector_field)
    } else {
        None
    };
    if let Some(column) = direct {
        let context = FilterContext {
            mode,
            schema: schema.as_ref(),
            skipped_field,
        };
        let page_rows = super::exact_direct::direct_page_rows(spec.query.len());
        let mut state = super::exact_direct::DirectScan {
            cursor: &mut cursor,
            column: &column,
            ranker: &mut ranker,
            scanned: &mut scanned_count,
        };
        if !super::exact_direct::rank_direct_pages(
            cassie,
            &mut state,
            (request, &context),
            page_rows,
        )? {
            return Ok(StreamOutcome::Declined);
        }
    } else {
        let page_rows = exact_page_rows(spec.query.len());
        loop {
            let documents = cursor.next_documents(&cassie.midge, page_rows, request.controls)?;
            if documents.is_empty() {
                break;
            }
            for mut document in documents {
                super::super::super::check_timeout(request.controls)?;
                scanned_count = scanned_count.saturating_add(1);
                let filter = FilterContext {
                    mode,
                    schema: schema.as_ref(),
                    skipped_field,
                };
                let FilterOutcome::Accepted(detached) =
                    filter_document(&mut document, request, &filter)?
                else {
                    continue;
                };
                let value = detached
                    .as_ref()
                    .or_else(|| document.payload.get(request.stored_vector_field))
                    .filter(|value| !value.is_null());
                let vector = match value {
                    None => None,
                    Some(value) => {
                        let Some(vector) = vector_from_json(value) else {
                            if mode == StreamMode::Candidates {
                                return Ok(StreamOutcome::Declined);
                            }
                            return Err(QueryError::General(
                                "exact vector candidate is invalid".to_string(),
                            ));
                        };
                        Some(vector)
                    }
                };
                if !ranker.rank(request.controls, &document.id, vector)? {
                    return Ok(StreamOutcome::Declined);
                }
            }
        }
    }
    finish_stream(cassie, request, ranker, scanned_count)
}

/// Ranks scored vectors into the bounded top-k heap and counts the rows it cannot score.
pub(super) struct Ranker<'a> {
    spec: &'a VectorDistanceTopKSpec,
    mode: StreamMode,
    top: AccountedVectorTopK,
    candidate_count: usize,
    unscored_count: usize,
}

impl<'a> Ranker<'a> {
    pub(super) fn try_new(
        spec: &'a VectorDistanceTopKSpec,
        mode: StreamMode,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        Ok(Self {
            spec,
            mode,
            top: AccountedVectorTopK::try_new(controls)?,
            candidate_count: 0,
            unscored_count: 0,
        })
    }

    /// Ranks one row. `false` means the generic executor must answer instead.
    pub(super) fn rank(
        &mut self,
        controls: &QueryExecutionControls,
        id: &str,
        vector: Option<Vec<f32>>,
    ) -> Result<bool, QueryError> {
        let Some(vector) = vector else {
            self.unscored_count = self.unscored_count.saturating_add(1);
            return Ok(true);
        };
        if self.mode == StreamMode::Candidates && !exact_vector_is_valid(self.spec, &vector) {
            return Ok(false);
        }
        validate_exact_vector(self.spec, &vector)?;
        let score = self.spec.exact_distance(&vector);
        self.candidate_count = self.candidate_count.saturating_add(1);
        self.top.try_push(
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&self.spec.direction, score),
                score,
                id: id.to_string(),
            },
            self.spec.top_needed(),
        )?;
        controls.scored_candidate(QueryScoringTestBoundary::ExactVector)?;
        Ok(true)
    }
}

fn finish_stream(
    cassie: &Cassie,
    request: &ExactVectorRequest<'_>,
    ranker: Ranker<'_>,
    scanned_count: usize,
) -> Result<StreamOutcome, QueryError> {
    if request.filter_expr.is_some() {
        cassie
            .runtime
            .record_vector_prefilter_usage(scanned_count, ranker.candidate_count, None);
    }
    super::super::super::check_timeout(request.controls)?;
    if ranker.mode == StreamMode::Candidates
        && ranker.unscored_count > 0
        && ranker.candidate_count < ranker.spec.top_needed()
    {
        // The generic ordering appends NULL-vector rows after the scored ones.
        return Ok(StreamOutcome::Declined);
    }
    let candidate_count = ranker.candidate_count;
    let (top, memory) = ranker.top.into_parts();
    Ok(StreamOutcome::Selected(
        top,
        memory,
        ScanCounts {
            scanned: scanned_count,
            scored: candidate_count,
        },
    ))
}

/// What the per-document filter step needs besides the document and the request.
pub(super) struct FilterContext<'a> {
    pub(super) mode: StreamMode,
    pub(super) schema: Option<&'a crate::catalog::CollectionSchema>,
    pub(super) skipped_field: Option<&'a str>,
}

pub(super) enum FilterOutcome {
    Rejected,
    /// Carries the stored vector when a general query shape detached it so its typed row could be
    /// built without the elements.
    Accepted(Option<serde_json::Value>),
}

/// Applies the filter to one document. A general query shape evaluates it on typed rows exactly
/// as the generic executor does.
pub(super) fn filter_document(
    document: &mut crate::midge::adapter::DocumentRef,
    request: &ExactVectorRequest<'_>,
    filter: &FilterContext<'_>,
) -> Result<FilterOutcome, QueryError> {
    let mut detached = None;
    let matches = match (filter.mode, request.filter_expr) {
        (StreamMode::Candidates, Some(filter_expr)) => {
            detached = document
                .payload
                .as_object_mut()
                .and_then(|payload| payload.remove(request.stored_vector_field));
            typed_row_matches(document, filter.schema, filter_expr, request)?
        }
        _ => document_matches_filter(
            document,
            request.filter_expr,
            filter.skipped_field,
            request.params,
            request.user_functions,
            request.session,
        )?,
    };
    Ok(if matches {
        FilterOutcome::Accepted(detached)
    } else {
        FilterOutcome::Rejected
    })
}

fn typed_row_matches(
    document: &crate::midge::adapter::DocumentRef,
    schema: Option<&crate::catalog::CollectionSchema>,
    filter_expr: &Expr,
    request: &ExactVectorRequest<'_>,
) -> Result<bool, QueryError> {
    let row = scan::document_to_row(document, schema);
    let row = scan::hydrate_character_arrays(row, request.controls)?;
    filter::filter_rows(
        vec![row],
        filter_expr,
        request.params,
        None,
        request.user_functions,
        request.session,
    )
    .map(|rows| !rows.is_empty())
}

fn exact_vector_is_valid(spec: &VectorDistanceTopKSpec, vector: &[f32]) -> bool {
    !vector.is_empty() && vector.len() == spec.query.len()
}

fn document_matches_filter(
    document: &crate::midge::adapter::DocumentRef,
    filter_expr: Option<&Expr>,
    skipped_field: Option<&str>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<bool, QueryError> {
    let Some(filter_expr) = filter_expr else {
        return Ok(true);
    };
    // `filter_expr` was already rewritten (see
    // `planner::logical::rewrite_reserved_id_references`) to reference `_id`
    // instead of `id` whenever the schema has no `id` field of its own, so
    // pushing `_id` here and letting the real payload (which only has an
    // `id` entry when the schema declares one) flow through unfiltered
    // resolves either case correctly without needing schema access here.
    let mut entries = vec![(
        crate::types::row_identity::ROW_IDENTITY_COLUMN.to_string(),
        Value::String(document.id.clone()),
    )];
    if let Some(payload) = document.payload.as_object() {
        // A supported prefilter never references the vector column, so its elements are not converted.
        entries.extend(
            payload
                .iter()
                .filter(|(name, _)| Some(name.as_str()) != skipped_field)
                .map(|(name, value)| {
                    (
                        name.clone(),
                        super::super::super::projected_read::json_to_query_value(value),
                    )
                }),
        );
    }
    filter::filter_rows(
        vec![BatchRow::new(entries)],
        filter_expr,
        params,
        None,
        user_functions,
        session,
    )
    .map(|rows| !rows.is_empty())
}

pub(super) fn validate_exact_vector(
    spec: &VectorDistanceTopKSpec,
    vector: &[f32],
) -> Result<(), QueryError> {
    if vector.len() != spec.query.len() || vector.is_empty() {
        return Err(QueryError::General(format!(
            "vector field '{}' on collection '{}' does not match query dimensions {}",
            spec.vector_field,
            spec.collection,
            spec.query.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::exact_page_rows;

    #[test]
    fn should_size_exact_pages_to_the_in_flight_byte_target() {
        // Arrange
        let widths = [1usize, 3, 384, 1_536, 16_000];

        // Act
        let pages = widths.map(exact_page_rows);

        // Assert
        assert_eq!(pages, [256, 256, 5, 1, 1]);
    }
}
