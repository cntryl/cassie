#[path = "sql_queries/types/numeric/mixed_numeric_comparisons.rs"]
mod mixed_numeric_comparisons;
#[path = "sql_queries/types/numeric/mixed_numeric_join_contracts.rs"]
mod mixed_numeric_joins;
#[path = "support/mixed_numeric.rs"]
mod support_mixed_numeric;

#[path = "sql_queries/parameters/parameter_signatures.rs"]
mod sql_parameter_signatures;

#[path = "support/alias_fixture.rs"]
mod support_alias_fixture;

#[path = "sql_queries/expressions/alias_integration.rs"]
mod alias_integration;

#[path = "sql_queries/expressions/conditional/conditional_expressions.rs"]
mod sql_conditional_expressions;
#[path = "sql_queries/expressions/constant_planning.rs"]
mod sql_constant_planning;

#[path = "sql_queries/expressions/conditional/coalesce_promotion.rs"]
mod sql_coalesce_promotion;

// Consolidated integration suite: sql_queries.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/relational_evidence.rs"]
mod support_relational_evidence;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;
#[path = "support/wire_queries.rs"]
mod support_type_metadata_contract;

#[path = "sql_queries/expressions/boolean_contexts.rs"]
mod sql_boolean_contexts;

#[path = "sql_queries/type_resolution/output_wildcard_boundary.rs"]
mod sql_type_output_wildcard_boundary;

#[path = "sql_queries/expressions/pagination.rs"]
mod sql_pagination;

#[path = "sql_queries/expressions/aliases.rs"]
mod sql_aliases;

#[path = "sql_queries/mod.rs"]
mod sql_queries;

#[path = "sql_queries/relational/relational_promotion_evidence.rs"]
mod relational_promotion_evidence;

// Formerly tests/integration_sql_aggregates.rs.
#[path = "sql_queries/aggregates/semantics/mod.rs"]
mod integration_sql_aggregates;

// Formerly tests/integration_sql_ctes.rs.
#[path = "sql_queries/ctes/semantics.rs"]
mod integration_sql_ctes;

// Formerly tests/integration_sql_explain.rs.
#[path = "sql_queries/planning/explain.rs"]
mod integration_sql_explain;
// Formerly tests/integration_sql_join_plans.rs.
#[path = "sql_queries/joins/join_plans.rs"]
mod integration_sql_join_plans;

// Formerly tests/integration_sql_joins.rs.
#[path = "sql_queries/joins/semantics.rs"]
mod integration_sql_joins;

// Formerly tests/integration_sql_null_semantics.rs.
#[path = "sql_queries/comparisons/null_semantics.rs"]
mod integration_sql_null_semantics;

// A column qualified by the query's own single relation (`t.col FROM t`)
// names the same column as the bare spelling on every read path.
#[path = "sql_queries/names/own_qualified_references.rs"]
mod own_qualified_references;

// Formerly tests/integration_sql_ordering.rs.
#[path = "sql_queries/ordering/semantics.rs"]
mod integration_sql_ordering;

// Formerly tests/integration_sql_predicates.rs.
#[path = "sql_queries/predicates/predicates.rs"]
mod integration_sql_predicates;

// Formerly tests/integration_sql_projection.rs.
#[path = "sql_queries/projection/semantics.rs"]
mod integration_sql_projection;

// Formerly tests/integration_sql_read_path_depth.rs.
#[path = "sql_queries/planning/read_path_depth.rs"]
mod integration_sql_read_path_depth;

// Formerly tests/integration_sql_scalar_functions.rs.
#[path = "sql_queries/functions/scalar_functions.rs"]
mod integration_sql_scalar_functions;
// Formerly tests/integration_sql_sets.rs.
#[path = "sql_queries/expressions/conditional/case.rs"]
mod integration_sql_case;

// PostgreSQL-aligned numeric result typing: literal typing, CASE result
// unification, COUNT, integer division, and overflow reporting.
#[path = "sql_queries/type_resolution/numeric_typing.rs"]
mod integration_sql_numeric_typing;

// Formerly tests/integration_sql_sets.rs.
#[path = "sql_queries/set_operations/sets.rs"]
mod integration_sql_sets;

// Formerly tests/integration_sql_table_free.rs.
#[path = "sql_queries/table_free/semantics.rs"]
mod integration_sql_table_free;

// Formerly tests/recursive_cte_semantics.rs.
#[path = "sql_queries/ctes/recursive_cte_semantics.rs"]
mod recursive_cte_semantics;

// Formerly tests/relational_semantic_correctness.rs.
#[path = "sql_queries/relational/relational_semantic_correctness.rs"]
mod relational_semantic_correctness;

// Formerly tests/scalar_functions.rs.
#[path = "sql_queries/functions/scalar_functions/runtime.rs"]
mod scalar_functions;

// Formerly tests/sql_semantic_regressions.rs.
#[path = "sql_queries/expressions/sql_semantic_regressions.rs"]
mod sql_semantic_regressions;
// Formerly tests/window_frames.rs.
#[path = "sql_queries/windows/window_frames.rs"]
mod window_frames;

// Formerly tests/sql_typed_literal_predicates.rs.
#[path = "sql_queries/casts/sql_typed_literal_predicates.rs"]
mod sql_typed_literal_predicates;

// Formerly tests/typed_literal_canonicalization.rs.
#[path = "sql_queries/casts/typed_literal_canonicalization.rs"]
mod typed_literal_canonicalization;

// Promotion evidence for the "Types and casts" row in docs/feature-support.md,
// tracked in docs/query-promotion-evidence.md. A boundary-value matrix rather
// than a seeded differential fixture: CAST has no alternate access path to
// compare against, so evidence here is exact-value and exact-error coverage
// at every documented type's representable range and rejection boundary.
#[path = "sql_queries/casts/cast_promotion_evidence.rs"]
mod type_cast_promotion_evidence;

// Regressions for issue #276. Rows carry the internal document identity only
// under the reserved `_id` key, and a bare `id` is rewritten to it at the
// logical-plan level (see `planner::logical::reserved_id`). That rewrite has
// to reach every source shape: a plan built anywhere but the planner's own
// entry points once missed it, and `id` then resolved to nothing at all —
// silently NULL, including into a table on `INSERT ... SELECT`.
#[path = "sql_queries/names/reserved_id_source_resolution.rs"]
mod reserved_id_source_resolution;

// Dropping a declared `id` column is newly permitted, and it moves a table
// from "declares its own `id`" to "resolves `id` to the internal identity"
// while its documents were written under the old schema. `SELECT *` must
// still report exactly one `id` column and a row of matching width; a row
// wider than its own column list desynchronizes the pgwire
// RowDescription/DataRow pair.
#[path = "sql_queries/names/reserved_id_payload_keys.rs"]
mod reserved_id_payload_keys;

// `_id` is the documented, always-available name for the internal row
// identity. A bare `id` means that identity only against a base table that
// declares no `id` column of its own; a derived relation that does not
// project `id` has no such column and must reject the reference.
#[path = "sql_queries/names/reserved_id_row_identity_column.rs"]
mod reserved_id_row_identity_column;

// `ALTER TABLE ... ADD COLUMN id` changes what a bare `id` means for the
// table: the new ordinary column shadows the internal-identity alias, and
// rows written before the change read back NULL for it. `_id` keeps naming
// the identity throughout, and dropping the column restores the alias.
#[path = "sql_queries/names/reserved_id_alter_table_shadowing.rs"]
mod reserved_id_alter_table_shadowing;

// ORDER BY <ordinal> sorts by the n-th output column of the SELECT list.
#[path = "sql_queries/ordering/order_by_ordinals.rs"]
mod order_by_ordinals;

#[path = "sql_queries/grammar/identifiers/case_resolution.rs"]
mod identifier_case_resolution;

#[path = "support/typed_join.rs"]
mod support_typed_join;
#[path = "sql_queries/joins/typed_join.rs"]
mod typed_join;
