// Integration suite: the reserved `cassie_graph` schema and path carrier types (#1268).

use cassie_test_support::{
    support_composite_types, support_pgwire, support_sql, support_type_metadata_contract,
};

#[path = "graph_types/fixture.rs"]
mod fixture;

#[path = "graph_types/catalog.rs"]
mod catalog;

#[path = "graph_types/constructors.rs"]
mod constructors;

#[path = "graph_types/refs.rs"]
mod refs;

#[path = "graph_types/refs_cost.rs"]
mod refs_cost;

#[path = "graph_types/paths.rs"]
mod paths;

#[path = "graph_types/reserved.rs"]
mod reserved;

#[path = "graph_types/storage.rs"]
mod storage;

#[path = "graph_types/wire.rs"]
mod wire;

#[path = "graph_types/projected_paths.rs"]
mod projected_paths;

#[path = "graph_types/parameter_functions.rs"]
mod parameter_functions;

#[path = "graph_types/record_frame.rs"]
mod record_frame;

#[path = "graph_types/record_codec.rs"]
mod record_codec;

#[path = "graph_types/driver.rs"]
mod driver;

#[path = "graph_types/images.rs"]
mod images;

#[path = "graph_types/clone_workflow.rs"]
mod clone_workflow;

#[path = "graph_types/clone_clients.rs"]
mod clone_clients;

#[path = "graph_types/clone_restart.rs"]
mod clone_restart;

#[path = "graph_types/driver_sqlx.rs"]
mod driver_sqlx;

#[path = "graph_types/argument_binding.rs"]
mod argument_binding;

#[path = "graph_types/reference_order.rs"]
mod reference_order;

#[path = "graph_types/identity_fixture.rs"]
mod identity_fixture;

#[path = "graph_types/identity_matrix.rs"]
mod identity_matrix;

#[path = "graph_types/identity_corrections.rs"]
mod identity_corrections;

#[path = "graph_types/identity_access.rs"]
mod identity_access;

#[path = "graph_types/identity_restart.rs"]
mod identity_restart;
