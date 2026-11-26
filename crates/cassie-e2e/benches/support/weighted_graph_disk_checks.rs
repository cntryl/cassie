//! Counter controls for the reopened weighted graph representative.

use super::graph_evidence::{Counts, Evidence};

pub fn should_validate_and_expand_the_weighted_graph_after_reopen(
    evidence: &mut Evidence,
    expected_edges: u64,
    cold: Counts,
    warm: Counts,
) {
    // Arrange: the caller pins the graph and executes the first and second query
    // on one reopened engine, with its result cache disabled.

    // Act: compare each invocation independently.
    for (scope, counts) in [("reopened", cold), ("warm", warm)] {
        // Assert: complete weight validation and indexed expansion in both scopes.
        if counts.validation_rows != expected_edges
            || counts.index_entries == 0
            || counts.neighborhoods == 0
            || counts.edges_admitted == 0
            || counts.full_rows != 0
            || counts.result_cache_hits != 0
        {
            evidence.finding(format!(
                "weighted routes ({scope}): expected {expected_edges} validated edges and indexed expansion without a result-cache hit; observed {counts:?}"
            ));
        }
    }
}
