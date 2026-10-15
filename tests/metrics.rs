// Consolidated integration suite: metrics.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/adaptive_evidence.rs"]
mod support_adaptive_evidence;
#[path = "support/metrics.rs"]
mod support_metrics;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/teardown_evidence.rs"]
mod support_teardown_evidence;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/metrics_adaptive.rs.
#[path = "metrics/adaptive/semantics/mod.rs"]
mod metrics_adaptive;

// Formerly tests/metrics_capacity.rs.
#[path = "metrics/capacity/semantics.rs"]
mod metrics_capacity;

// Formerly tests/metrics_feedback.rs.
#[path = "metrics/feedback/semantics/mod.rs"]
mod metrics_feedback;
// Formerly tests/metrics_joins.rs.
#[path = "metrics/joins/semantics.rs"]
mod metrics_joins;

// Formerly tests/metrics_plan_pgwire.rs.
#[path = "metrics/plan/plan_pgwire.rs"]
mod metrics_plan_pgwire;

// Formerly tests/metrics_read_paths.rs.
#[path = "metrics/read_paths/semantics.rs"]
mod metrics_read_paths;

// Formerly tests/metrics_runtime.rs.
#[path = "metrics/runtime/semantics.rs"]
mod metrics_runtime;

// Formerly tests/metrics_runtime_projections.rs.
#[path = "metrics/runtime/runtime_projections.rs"]
mod metrics_runtime_projections;

// Formerly tests/metrics_search.rs.
#[path = "metrics/search/semantics.rs"]
mod metrics_search;
