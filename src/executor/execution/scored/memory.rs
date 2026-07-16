use super::{
    BatchRow, BinaryHeap, HashSet, QueryError, QueryExecutionControls, ScoredSearchCandidate,
    TokenizedFulltextDocument,
};

type Reservation = crate::runtime::QueryMemoryReservation;

pub(super) fn batch_row_bytes(row: &BatchRow) -> usize {
    row.entries()
        .iter()
        .map(|(name, value)| name.len().saturating_add(format!("{value:?}").len()))
        .sum()
}

pub(super) fn reserve_fulltext_documents(
    controls: &QueryExecutionControls,
    documents: &[TokenizedFulltextDocument],
) -> Result<Reservation, QueryError> {
    let bytes = documents
        .iter()
        .map(|document| {
            document.id.len().saturating_add(
                serde_json::to_vec(&document.text_stats)
                    .map(|bytes| bytes.len())
                    .unwrap_or_default(),
            )
        })
        .sum();
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn reserve_candidate_ids(
    controls: &QueryExecutionControls,
    candidates: Option<&HashSet<String>>,
) -> Result<Reservation, QueryError> {
    let bytes = candidates.into_iter().flatten().map(String::len).sum();
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn replace_scored_candidates(
    previous: Option<Reservation>,
    controls: &QueryExecutionControls,
    candidates: &BinaryHeap<ScoredSearchCandidate>,
) -> Result<Reservation, QueryError> {
    drop(previous);
    let bytes = candidates
        .iter()
        .map(|candidate| {
            candidate
                .id
                .len()
                .saturating_add(std::mem::size_of::<ScoredSearchCandidate>())
        })
        .sum();
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn reserve_scored_candidates(
    controls: &QueryExecutionControls,
    candidates: &BinaryHeap<ScoredSearchCandidate>,
) -> Result<Reservation, QueryError> {
    replace_scored_candidates(None, controls, candidates)
}
