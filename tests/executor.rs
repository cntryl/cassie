// Consolidated integration suite: executor.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/admission_control.rs.
#[path = "executor/resources/admission_control.rs"]
mod admission_control;

// Formerly tests/application_pipeline_schema.rs.
#[path = "executor/ddl/application_pipeline_schema.rs"]
mod application_pipeline_schema;

// Formerly tests/executor_commands.rs.
#[path = "executor/commands/semantics.rs"]
mod executor_commands;

// Formerly tests/executor_fulltext_scoring.rs.
#[path = "executor/search/fulltext_scoring.rs"]
mod executor_fulltext_scoring;
// Formerly tests/executor_hybrid_scoring.rs.
#[path = "executor/hybrid/hybrid_scoring.rs"]
mod executor_hybrid_scoring;

// Formerly tests/executor_limits.rs.
#[path = "executor/resources/limits.rs"]
mod executor_limits;

// Formerly tests/executor_parallel.rs.
#[path = "executor/parallel/mod.rs"]
mod executor_parallel;

// Formerly tests/executor_projection.rs.
#[path = "executor/projection/semantics.rs"]
mod executor_projection;

// Formerly tests/executor_query_sources.rs.
#[path = "executor/sources/query_sources.rs"]
mod executor_query_sources;

// Formerly tests/executor_sort.rs.
#[path = "executor/ordering/sort.rs"]
mod executor_sort;
// Formerly tests/executor_streaming.rs.
#[path = "executor/streaming/semantics.rs"]
mod executor_streaming;

// Formerly tests/executor_vector_scoring.rs.
#[path = "executor/vectors/vector_scoring.rs"]
mod executor_vector_scoring;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "executor/relational/optimized_read_equivalence.rs"]
mod optimized_read_equivalence;

#[path = "executor/joins/join_scope_and_shape.rs"]
mod join_scope_and_shape;

#[path = "support/array_order.rs"]
mod support_array_order;

#[path = "executor/ordering/typed_array_ordering.rs"]
mod typed_array_ordering;

#[path = "executor/relational/typed_relational_phase_equivalence.rs"]
mod typed_relational_phase_equivalence;

#[path = "executor/relational/typed_alias_bound_integration.rs"]
mod typed_alias_bound_integration;
