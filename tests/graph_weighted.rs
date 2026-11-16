// Integration suite: graph_weighted_paths (#1258).
// Subsystem files live under `graph_weighted/`; shared fixtures stay in `support/`.

use cassie_test_support::support_pgwire;
use cassie_test_support::support_sql;
use cassie_test_support::support_type_metadata_contract;

#[path = "support/graph_oracle/mod.rs"]
mod graph_oracle;

#[path = "graph_weighted/world.rs"]
mod world;

#[path = "graph_weighted/arguments.rs"]
mod arguments;

#[path = "graph_weighted/binding.rs"]
mod binding;

#[path = "graph_weighted/oracle.rs"]
mod oracle;

#[path = "graph_weighted/views.rs"]
mod views;

#[path = "graph_weighted/security.rs"]
mod security;

#[path = "graph_weighted/resources.rs"]
mod resources;

#[path = "graph_weighted/matrix.rs"]
mod matrix;
