// Integration suite: the GRAPH_TABLE table source (#1256).
// Subsystem files live under `graph_table/`; shared fixtures stay in `support/`.

use cassie_test_support::support_pgwire;
use cassie_test_support::support_type_metadata_contract;

#[path = "graph_table/wire.rs"]
mod wire;
