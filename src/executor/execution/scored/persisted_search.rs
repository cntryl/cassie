//! Candidate retrieval from the persisted full-text artifacts for both the
//! `'terms'` and `'query'` syntaxes.

use super::{filter, Cassie, QueryError, QueryExecutionControls};
use crate::midge::adapter::fulltext_retrieval::PersistedFulltextCandidateSet;
use crate::runtime::QueryMemoryReservation;
use crate::search::analyzer::AnalyzerConfig;
use crate::search::query::{CompiledQuery, SearchQueryInput, SearchSyntax};

pub(super) struct PersistedSearchCandidates {
    pub(super) candidates: PersistedFulltextCandidateSet,
    pub(super) search: filter::PreparedSearch,
    pub(super) memory: Vec<QueryMemoryReservation>,
}

pub(super) enum PersistedSearchLoad {
    Ready(Box<PersistedSearchCandidates>),
    /// The persisted artifacts cannot answer; run the authoritative path.
    Exact(&'static str),
}

fn is_query_control_error(error: &crate::app::CassieError) -> bool {
    matches!(
        error,
        crate::app::CassieError::QueryCanceled
            | crate::app::CassieError::DeadlineExceeded
            | crate::app::CassieError::ResourceLimit(_)
    )
}

fn persisted_fallback_reason(error: &crate::app::CassieError) -> &'static str {
    if error.to_string().contains("missing_candidate_row") {
        "missing_candidate_row"
    } else {
        "invalid_persisted_artifact"
    }
}

/// Reads the candidates of `input` from the persisted index `index_name`.
///
/// Cancellation, deadline and resource-limit errors (including the query
/// expansion limits) are returned; any other artifact problem selects the
/// authoritative path.
pub(super) fn load_persisted_search_candidates(
    cassie: &Cassie,
    collection: &str,
    index_name: &str,
    input: &SearchQueryInput,
    analyzer: &AnalyzerConfig,
    controls: &QueryExecutionControls,
) -> Result<PersistedSearchLoad, QueryError> {
    match input.syntax {
        SearchSyntax::Terms => {
            let query_terms = filter::prepare_query_terms_with_analyzer(&input.text, analyzer);
            match cassie.midge.fulltext_candidate_set_controlled(
                collection,
                index_name,
                &query_terms,
                controls,
            ) {
                Ok(candidates) => {
                    let (candidates, memory) = candidates.into_parts();
                    Ok(PersistedSearchLoad::Ready(Box::new(
                        PersistedSearchCandidates {
                            candidates,
                            search: filter::PreparedSearch::Terms(query_terms),
                            memory: vec![memory],
                        },
                    )))
                }
                Err(error) if is_query_control_error(&error) => Err(QueryError::from(error)),
                Err(error) => Ok(PersistedSearchLoad::Exact(persisted_fallback_reason(&error))),
            }
        }
        SearchSyntax::Query => {
            let compiled = CompiledQuery::compile(input, analyzer)?;
            match cassie.midge.fulltext_candidate_set_for_query_controlled(
                collection,
                index_name,
                &compiled,
                controls,
            ) {
                Ok((candidates, resolved, query_memory)) => {
                    let (candidates, memory) = candidates.into_parts();
                    Ok(PersistedSearchLoad::Ready(Box::new(
                        PersistedSearchCandidates {
                            candidates,
                            search: filter::PreparedSearch::Query(resolved),
                            memory: vec![memory, query_memory],
                        },
                    )))
                }
                Err(error) if is_query_control_error(&error) => Err(QueryError::from(error)),
                Err(error) => Ok(PersistedSearchLoad::Exact(persisted_fallback_reason(&error))),
            }
        }
    }
}
