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

#[path = "graph_types/driver_sqlx.rs"]
mod driver_sqlx;
