// Consolidated integration suite: vector_embeddings.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/retrieval.rs"]
mod support_retrieval;
use cassie_test_support::support_sql;
use cassie_test_support::support_temp_dirs;

#[path = "vector_embeddings/vectors/ann_candidate_exhaustion.rs"]
mod ann_candidate_exhaustion;

// #1365: network-free checks of the opt-in embedding workload machinery.
#[path = "vector_embeddings/embeddings/embedding_workloads.rs"]
mod embedding_workloads;

#[path = "vector_embeddings/vectors/exact_vector_cost.rs"]
mod exact_vector_cost;

// Formerly tests/embedding_local.rs.
#[path = "vector_embeddings/embeddings/embedding_local.rs"]
mod embedding_local;

// Formerly tests/embedding_response_limits.rs.
#[path = "vector_embeddings/embeddings/embedding_response_limits.rs"]
mod embedding_response_limits;

// Formerly tests/embedding_self_hosted.rs.
#[path = "vector_embeddings/embeddings/embedding_self_hosted.rs"]
mod embedding_self_hosted;

// Formerly tests/embedding_validation.rs.
#[path = "vector_embeddings/embeddings/embedding_validation.rs"]
mod embedding_validation;

// Seeded insert/replace/delete/abort/rebuild churn over HNSW and IVFFlat publication.
#[path = "vector_embeddings/indexes/churn/mod.rs"]
mod vector_index_churn;

// Formerly tests/hnsw_indexes.rs.
#[path = "vector_embeddings/indexes/hnsw/hnsw_indexes/mod.rs"]
mod hnsw_indexes;

// Formerly tests/integration_sql_hybrid_query.rs.
#[path = "vector_embeddings/hybrid/hybrid_query.rs"]
mod integration_sql_hybrid_query;

// Formerly tests/integration_sql_vector_indexes.rs.
#[path = "vector_embeddings/vectors/vector_indexes.rs"]
mod integration_sql_vector_indexes;

// Formerly tests/integration_sql_vector_query.rs.
#[path = "vector_embeddings/vectors/vector_query.rs"]
mod integration_sql_vector_query;

// Formerly tests/ivfflat_completeness.rs.
#[path = "vector_embeddings/indexes/ivfflat/ivfflat_completeness.rs"]
mod ivfflat_completeness;
// Formerly tests/ivfflat_indexes.rs.
#[path = "vector_embeddings/indexes/ivfflat/ivfflat_indexes.rs"]
mod ivfflat_indexes;

// Formerly tests/normalized_vector_generation.rs.
#[path = "vector_embeddings/vectors/normalized_vector_generation.rs"]
mod normalized_vector_generation;

// Formerly tests/openai_provider.rs.
#[path = "vector_embeddings/providers/openai_provider.rs"]
mod openai_provider;

// Formerly tests/vector_index_metadata.rs.
#[path = "vector_embeddings/vectors/vector_index_metadata.rs"]
mod vector_index_metadata;

// Formerly tests/vector_query_stability.rs.
#[path = "vector_embeddings/vectors/vector_query_stability.rs"]
mod vector_query_stability;

// Formerly tests/vector_state_generation.rs.
#[path = "vector_embeddings/vectors/vector_state_generation.rs"]
mod vector_state_generation;

// A declared `id` column is an ordinary field; only `_id` names the internal
// row identity in the vector top-k fast path.
#[path = "vector_embeddings/vectors/vector_declared_id.rs"]
mod vector_declared_id;

#[path = "vector_embeddings/vectors/vector_source_column_lifecycle.rs"]
mod vector_source_column_lifecycle;

#[path = "vector_embeddings/vectors/vector_index_drop.rs"]
mod vector_index_drop;

#[path = "vector_embeddings/vectors/vector_index_write_stall.rs"]
mod vector_index_write_stall;

// Vector bind parameters written to columns that are not declared VECTOR(n).
#[path = "vector_embeddings/vectors/vector_parameter_finiteness.rs"]
mod vector_parameter_finiteness;

// Embedding writes: explicit vectors, source lifecycle, index backfill, literal spellings.
#[path = "vector_embeddings/vectors/vector_write_path.rs"]
mod vector_write_path;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "vector_embeddings/vectors/vector_read_equivalence.rs"]
mod vector_read_equivalence;

#[path = "support/backfill_provider.rs"]
mod backfill_provider;

#[path = "support/vector_publication.rs"]
mod vector_publication_support;

#[path = "vector_embeddings/vectors/metric_qualification.rs"]
mod vector_metric_qualification;
#[path = "support/vector_qualification.rs"]
mod vector_qualification_support;

#[path = "vector_embeddings/vectors/sql_ann_metrics.rs"]
mod sql_ann_metrics;

#[path = "vector_embeddings/vectors/sql_ann_metric_lifecycle.rs"]
mod sql_ann_metric_lifecycle;

#[path = "vector_embeddings/vectors/sql_ann_metric_recall.rs"]
mod sql_ann_metric_recall;

// Corpus-wide BM25 oracle and two-tenant pgwire fixture shared with the search suite.
#[path = "support/ranked_search.rs"]
mod support_ranked_search;

// Tenant-filtered hybrid ranking against the documented hybrid scoring contract.
#[path = "vector_embeddings/hybrid/tenant_hybrid_ranking.rs"]
mod tenant_hybrid_ranking;

// Scalar-filtered vector candidates (#1363): fixture, standalone and hybrid suites.
#[path = "support/vector_filtered.rs"]
mod support_vector_filtered;

#[path = "vector_embeddings/filtered/filtered_vector_selectivity.rs"]
mod filtered_vector_selectivity;

#[path = "vector_embeddings/filtered/filtered_vector_contract.rs"]
mod filtered_vector_contract;

#[path = "vector_embeddings/filtered/filtered_vector_snapshots.rs"]
mod filtered_vector_snapshots;

#[path = "vector_embeddings/hybrid/tenant_hybrid_candidates.rs"]
mod tenant_hybrid_candidates;
