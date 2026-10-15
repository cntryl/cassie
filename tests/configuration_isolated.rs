// Consolidated integration suite: configuration_isolated.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/environment.rs"]
mod support_environment;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/sprint01_runtime_baseline.rs.
#[path = "configuration_isolated/configuration/runtime_baseline.rs"]
mod sprint01_runtime_baseline;

// Formerly tests/transport_boundaries.rs.
#[path = "configuration_isolated/configuration/transport_boundaries.rs"]
mod transport_boundaries;

// Formerly tests/embedding_provider_controls.rs.
#[path = "configuration_isolated/configuration/embedding_provider_controls.rs"]
mod embedding_provider_controls;
