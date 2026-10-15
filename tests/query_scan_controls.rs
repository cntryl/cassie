// Integration suites that exercise the process-global query scan test hook.
// Keeping them in one flat test target prevents unrelated scans from consuming
// an armed hook while preserving parallel setup through shared test guards.

#[path = "support/column_store_controls.rs"]
mod support_column_store_controls;
#[path = "support/graph_evidence.rs"]
mod support_graph_evidence;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "query_scan_controls/scans/normalized_query_controls.rs"]
mod rest_normalized_query_controls;

#[path = "query_scan_controls/scans/column_store_query_controls.rs"]
mod column_store_query_controls;

// Formerly tests/column_batch_controls.rs.
#[path = "query_scan_controls/scans/column_batch_controls.rs"]
mod column_batch_controls;

// Formerly tests/fulltext_filtered_controls.rs.
#[path = "query_scan_controls/predicates/fulltext_filtered_controls.rs"]
mod fulltext_filtered_controls;

// Formerly tests/scalar_index_resource_controls.rs.
#[path = "query_scan_controls/resources/scalar_index_resource_controls.rs"]
mod scalar_index_resource_controls;

// Formerly tests/vector_ann_concurrency.rs.
#[path = "query_scan_controls/scans/vector_ann_concurrency.rs"]
mod vector_ann_concurrency;

// Formerly tests/rest_request_cancellation.rs.
#[path = "query_scan_controls/cancellation/request_cancellation.rs"]
mod rest_request_cancellation;

// Formerly tests/graph_resilience.rs.
#[path = "query_scan_controls/scans/graph_resilience.rs"]
mod graph_resilience;

// Formerly tests/query_resource_controls.rs.
#[path = "query_scan_controls/resources/query_resource_controls.rs"]
mod query_resource_controls;

// Formerly tests/specialized_query_controls.rs.
#[path = "query_scan_controls/scans/specialized_query_controls.rs"]
mod specialized_query_controls;

// Formerly tests/specialized_query_controls_retrieval.rs.
#[path = "query_scan_controls/scans/specialized_query_controls_retrieval.rs"]
mod specialized_query_controls_retrieval;

#[path = "query_scan_controls/column_store/row_limits.rs"]
mod column_limit;

#[path = "support/column_projection_fixture.rs"]
mod support_column_projection_fixture;

#[path = "support/teardown_evidence.rs"]
mod support_teardown_evidence;

#[path = "support/retrieval_directory.rs"]
mod support_retrieval_directory;

#[path = "support/retrieval_phase_evidence.rs"]
mod support_retrieval_phase_evidence;
