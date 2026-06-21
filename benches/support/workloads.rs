#![allow(dead_code)]

#[path = "workloads/context.rs"]
mod context;
#[path = "workloads/hotpath.rs"]
mod hotpath;
#[path = "workloads/http.rs"]
mod http;
#[path = "workloads/pgwire.rs"]
mod pgwire;
#[path = "workloads/sql.rs"]
mod sql;
#[path = "workloads/system.rs"]
mod system;

pub use context::{
    context, context_with_mock_tei_embeddings, empty_context, runtime, BenchContext,
};
pub use hotpath::*;
pub use http::*;
pub use pgwire::*;
pub use sql::*;
pub use system::*;
