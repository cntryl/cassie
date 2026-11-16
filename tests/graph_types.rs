// Integration suite: the reserved `cassie_graph` schema and path carrier types (#1268).

use cassie_test_support::support_sql;

#[path = "graph_types/fixture.rs"]
mod fixture;

#[path = "graph_types/catalog.rs"]
mod catalog;

#[path = "graph_types/constructors.rs"]
mod constructors;

#[path = "graph_types/scratch.rs"]
mod scratch;
