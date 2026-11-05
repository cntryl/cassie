// Consolidated integration suite: compatibility.
// Shared fixtures live in tests/support; former test targets remain named modules.

use cassie_test_support::support_pgwire;

use cassie_test_support::support_sql;

// Formerly tests/compatibility_matrix.rs.
#[path = "compatibility/clients/tokio_postgres/matrix/mod.rs"]
mod compatibility_matrix;

// Formerly tests/compatibility_sqlalchemy.rs.
#[path = "compatibility/clients/sqlalchemy/semantics.rs"]
mod compatibility_sqlalchemy;

// Formerly tests/compatibility_diesel.rs.
#[path = "compatibility/clients/diesel/semantics.rs"]
mod compatibility_diesel;

// Formerly tests/compatibility_sqlx.rs.
#[path = "compatibility/clients/sqlx/semantics.rs"]
mod compatibility_sqlx;

#[path = "support/desktop_trace.rs"]
mod support_desktop_trace;

#[path = "compatibility/clients/desktop_traces/desktop_client_trace_replay.rs"]
mod desktop_client_trace_replay;

#[path = "compatibility/mod.rs"]
mod compatibility;

#[path = "support/unique_conflict.rs"]
mod support_unique_conflict;

#[path = "support/scalar_index_rebuild.rs"]
mod support_scalar_index_rebuild;
