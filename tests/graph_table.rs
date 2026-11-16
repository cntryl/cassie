// Integration suite: the GRAPH_TABLE table source (#1256, #1257).
// Subsystem files live under `graph_table/`; shared fixtures stay in `support/`.

use cassie_test_support::support_pgwire;
use cassie_test_support::support_sql;
use cassie_test_support::support_sql_fixture;
use cassie_test_support::support_type_metadata_contract;

#[path = "support/graph_oracle/mod.rs"]
mod graph_oracle;

#[path = "graph_table/world.rs"]
mod world;

#[path = "graph_table/exact_reads.rs"]
mod exact_reads;

#[path = "graph_table/transactions.rs"]
mod transactions;

#[path = "graph_table/composition.rs"]
mod composition;

#[path = "graph_table/resources.rs"]
mod resources;

#[path = "graph_table/semantics.rs"]
mod semantics;

#[path = "graph_table/matrix.rs"]
mod matrix;

#[path = "graph_table/wire.rs"]
mod wire;

#[path = "graph_table/paths.rs"]
mod paths;

#[path = "graph_table/database_scope.rs"]
mod database_scope;

#[path = "graph_table/persistence.rs"]
mod persistence;

#[path = "graph_table/variants.rs"]
mod variants;

#[path = "graph_table/fast_reads.rs"]
mod fast_reads;
