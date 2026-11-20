//! Strategy selection for vector candidates restricted by an equality filter (#1363).
//!
//! The contract is documented in `docs/vector-filtered-search-contract.md`. A statement whose
//! `WHERE` clause contains `column = literal` conjuncts over eligible scalar indexes reads the
//! eligible id subset from those indexes first. The subset size then selects between a bounded
//! exact rerank over the subset and filtered ANN with adaptive windows. Every other shape, and
//! every attempt that cannot fill its window, falls back with a labelled reason.

use std::collections::HashSet;

use super::fulltext_filtered_topk::{conjuncts, intersect_scalar_conjuncts};
use super::{Cassie, CassieSession, Expr, QueryError, QueryExecutionControls, Value};
use crate::embeddings::{DistanceMetric, VectorIndexRecord, VectorIndexType};
use crate::runtime::QueryMemoryReservation;

#[path = "vector_filtered/ann.rs"]
pub(super) mod ann;

/// A subset of at most `top_needed * EXACT_SUBSET_PER_ROW` rows (floor [`EXACT_SUBSET_FLOOR`])
/// is reranked exactly; ANN windows would read comparable rows with weaker guarantees.
pub(super) const EXACT_SUBSET_PER_ROW: usize = 32;
pub(super) const EXACT_SUBSET_FLOOR: usize = 256;
/// Eligible ANN candidates retained for exact rerank, as a multiple of `OFFSET + LIMIT`.
pub(super) const ANN_RERANK_OVERSAMPLE: usize = 4;
/// First ANN window, as a multiple of `OFFSET + LIMIT` (floor [`INITIAL_WINDOW_FLOOR`]).
pub(super) const INITIAL_WINDOW_PER_ROW: usize = 16;
pub(super) const INITIAL_WINDOW_FLOOR: usize = 64;

/// The strategy a filtered statement runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilteredSelection {
    /// Exact rerank of every row in the scalar-index subset.
    BoundedExact,
    /// ANN windows grown until enough eligible candidates are found, then exact rerank.
    FilteredAnn,
    /// The authoritative full exact scan, with the labelled reason.
    FullFallback(&'static str),
}

/// Largest subset reranked exactly instead of probing ANN windows.
pub(super) fn bounded_exact_limit(top_needed: usize) -> usize {
    top_needed
        .saturating_mul(EXACT_SUBSET_PER_ROW)
        .max(EXACT_SUBSET_FLOOR)
}

/// Selects the strategy from the subset size. `adaptive_max` is the configured candidate cap.
pub(crate) fn select_strategy(
    subset_len: usize,
    top_needed: usize,
    adaptive_max: usize,
    ann_available: bool,
) -> FilteredSelection {
    if subset_len <= bounded_exact_limit(top_needed).min(adaptive_max) {
        FilteredSelection::BoundedExact
    } else if ann_available {
        FilteredSelection::FilteredAnn
    } else if subset_len <= adaptive_max {
        FilteredSelection::BoundedExact
    } else {
        FilteredSelection::FullFallback("subset-over-candidate-cap")
    }
}

/// The ANN index that can serve this query, if any. Mirrors the unfiltered ANN conditions:
/// ascending order, matching metric and a non-degenerate query.
pub(crate) fn usable_ann_index(
    cassie: &Cassie,
    collection: &str,
    stored_vector_field: &str,
    query: AnnQueryShape,
) -> Result<Option<VectorIndexRecord>, QueryError> {
    if !query.ascending {
        return Ok(None);
    }
    let Some(index) = cassie
        .midge
        .get_vector_index_definition(collection, stored_vector_field)?
    else {
        return Ok(None);
    };
    if !matches!(
        index.metadata.index_type,
        VectorIndexType::Hnsw | VectorIndexType::IvfFlat
    ) || index.metadata.metric != query.metric
        || (index.metadata.index_type == VectorIndexType::Hnsw && index.metadata.hnsw.is_none())
        || query.degenerate
    {
        return Ok(None);
    }
    Ok(Some(index))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AnnQueryShape {
    pub(crate) metric: DistanceMetric,
    pub(crate) ascending: bool,
    pub(crate) degenerate: bool,
}

/// The id subset admitted by the eligible scalar equality conjuncts of a filter.
pub(super) struct EligibleSubset {
    pub(super) ids: HashSet<String>,
    /// Held for the life of the subset so its ids stay admitted by the query budget.
    pub(super) _memory: Vec<QueryMemoryReservation>,
}

/// Probes every eligible scalar equality conjunct of `filter` and intersects the ids. `None`
/// means no conjunct is served by an eligible scalar index.
pub(super) fn eligible_subset(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    filter: &Expr,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<EligibleSubset>, QueryError> {
    let mut parts = Vec::new();
    conjuncts(filter, &mut parts);
    Ok(
        intersect_scalar_conjuncts(cassie, session, collection, &parts, params, controls)?.map(
            |found| EligibleSubset {
                ids: found.ids,
                _memory: found.memory,
            },
        ),
    )
}
