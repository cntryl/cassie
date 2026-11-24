#[doc(hidden)]
pub mod benchmark;
pub mod pgwire;
pub mod rest;
pub(crate) mod transport;
pub use cassie_engine::{
    app, catalog, config, embeddings, executor, hybrid, midge, planner, runtime, search, sql, types,
    vector,
};

pub use cassie_engine::{
    Cassie, CassieError, CassieRuntimeConfig, CassieRuntimeConfigState, CassieSession,
    DatabaseBackupStream, DatabaseRestoreSession,
};
