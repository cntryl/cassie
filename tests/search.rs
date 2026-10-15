// Consolidated integration suite: search.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/fulltext_index_completeness.rs.
#[path = "search/retrieval/fulltext_index_completeness.rs"]
mod fulltext_index_completeness;

// Formerly tests/fulltext_persisted_retrieval.rs.
#[path = "search/index_lifecycle/fulltext_persisted_retrieval.rs"]
mod fulltext_persisted_retrieval;

// Formerly tests/fulltext_persisted_sql.rs.
#[path = "search/index_lifecycle/fulltext_persisted_sql.rs"]
mod fulltext_persisted_sql;
// Formerly tests/fulltext_publication_recovery.rs.
#[path = "search/index_lifecycle/fulltext_publication_recovery.rs"]
mod fulltext_publication_recovery;

// Formerly tests/fulltext_retrieval_corruption.rs.
#[path = "search/retrieval/fulltext_retrieval_corruption.rs"]
mod fulltext_retrieval_corruption;

// Formerly tests/integration_sql_fulltext_query.rs.
#[path = "search/retrieval/fulltext_query.rs"]
mod integration_sql_fulltext_query;

// Formerly tests/search_vector.rs.
#[path = "search/retrieval/vector.rs"]
mod search_vector;

// A declared `id` column is an ordinary field; only `_id` names the internal
// row identity in the scored top-k fast path.
#[path = "search/retrieval/scored_declared_id.rs"]
mod scored_declared_id;

// Search and vector functions name their field as a column reference, which
// is case-insensitive like any other.
#[path = "search/retrieval/scored_field_case.rs"]
mod scored_field_case;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "search/retrieval/scored_read_equivalence.rs"]
mod scored_read_equivalence;

// Analyzer and corpus-statistics consistency for scored fulltext reads.
#[path = "search/retrieval/fulltext_analyzer_consistency.rs"]
mod fulltext_analyzer_consistency;
