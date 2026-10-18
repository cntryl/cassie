// Consolidated integration suite: planner_controls.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/metrics.rs"]
mod support_metrics;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/query_evidence.rs"]
mod support_query_evidence;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "planner_controls/cache/transaction_qualification.rs"]
mod cache_transaction_qualification;

// Formerly tests/adaptive_promotion_evidence.rs.
#[path = "planner_controls/planning/adaptive_promotion_evidence.rs"]
mod adaptive_promotion_evidence;

// Formerly tests/execution_result_cache.rs.
#[path = "planner_controls/planning/execution_result_cache.rs"]
mod execution_result_cache;

// Formerly tests/optimizer_memo.rs.
#[path = "planner_controls/planning/optimizer_memo.rs"]
mod optimizer_memo;

// Formerly tests/plan_cache.rs.
#[path = "planner_controls/planning/plan_cache/mod.rs"]
mod plan_cache;

// Formerly tests/planner_aggregates_sets.rs.
#[path = "planner_controls/aggregates/planner_aggregates_sets.rs"]
mod planner_aggregates_sets;

// Formerly tests/planner_commands.rs.
#[path = "planner_controls/planning/planner_commands.rs"]
mod planner_commands;

// Formerly tests/planner_costs.rs.
#[path = "planner_controls/planning/planner_costs.rs"]
mod planner_costs;

// Formerly tests/planner_estimates.rs.
#[path = "planner_controls/planning/planner_estimates.rs"]
mod planner_estimates;

// Formerly tests/planner_indexes.rs.
#[path = "planner_controls/indexes/planner_indexes.rs"]
mod planner_indexes;

// Formerly tests/planner_logical.rs.
#[path = "planner_controls/planning/planner_logical.rs"]
mod planner_logical;
// Formerly tests/planner_physical.rs.
#[path = "planner_controls/planning/planner_physical.rs"]
mod planner_physical;

// Formerly tests/planner_read_path_depth.rs.
#[path = "planner_controls/planning/planner_read_path_depth.rs"]
mod planner_read_path_depth;

// Formerly tests/query_cancellation.rs.
#[path = "planner_controls/cancellation/query_cancellation.rs"]
mod query_cancellation;

// Formerly tests/query_promotion_evidence.rs.
#[path = "planner_controls/planning/query_promotion_evidence.rs"]
mod query_promotion_evidence;

// Formerly tests/statement_routes.rs.
#[path = "planner_controls/planning/statement_routes.rs"]
mod statement_routes;
