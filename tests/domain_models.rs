// Consolidated integration suite: domain_models.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "domain_models/graph/qualification.rs"]
mod graph_completeness;
#[path = "support/graph.rs"]
mod support_graph;
#[path = "support/graph_neighbors.rs"]
mod support_graph_neighbors;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;
#[path = "support/time_series_evidence.rs"]
mod support_time_series_evidence;

// Formerly tests/graph_transaction_semantics.rs.
#[path = "domain_models/graph/transactions/graph_transaction_semantics.rs"]
mod graph_transaction_semantics;

// Formerly tests/integration_sql_graph.rs.
#[path = "domain_models/graph/queries/semantics.rs"]
mod integration_sql_graph;

// Formerly tests/time_series_index_completeness.rs.
#[path = "domain_models/time_series/indexes/time_series_index_completeness.rs"]
mod time_series_index_completeness;

// Formerly tests/time_series_indexes.rs.
#[path = "domain_models/time_series/indexes/time_series_indexes/mod.rs"]
mod time_series_indexes;

// Formerly tests/time_series_retention.rs.
#[path = "domain_models/time_series/retention/time_series_retention.rs"]
mod time_series_retention;
// Formerly tests/time_series_rollups.rs.
#[path = "domain_models/time_series/rollups/time_series_rollups/mod.rs"]
mod time_series_rollups;

#[path = "domain_models/graph/database_scope/graph_database_scope.rs"]
mod graph_database_scope;

#[path = "domain_models/graph/ddl/graph_backing_table_ddl.rs"]
mod graph_backing_table_ddl;

#[path = "domain_models/time_series/partitions/time_series_partition_delimiters.rs"]
mod time_series_partition_delimiters;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "domain_models/time_series/rollups/boolean_predicates.rs"]
mod rollup_boolean_predicates;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;

#[path = "domain_models/time_series/queries/column_time_series_read_equivalence.rs"]
mod column_time_series_read_equivalence;

#[path = "domain_models/graph/queries/graph_function_correctness.rs"]
mod graph_function_correctness;

#[path = "domain_models/time_series/qualification.rs"]
mod time_series_qualification;
