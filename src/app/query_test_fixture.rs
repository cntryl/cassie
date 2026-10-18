//! Shared private fixture for execution-context and datum-observer tests.
use super::{Cassie, CassieSession, QueryExecutionControls};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use std::path::PathBuf;
use std::time::Instant;

pub(super) struct Fixture {
    cassie: Option<Cassie>,
    pub(super) session: CassieSession,
    path: PathBuf,
}
impl Fixture {
    pub(super) fn new() -> Self {
        Self::with_config(CassieRuntimeConfig::from_env().expect("fixture config"))
    }
    pub(super) fn with_config(config: CassieRuntimeConfig) -> Self {
        let path = std::env::temp_dir().join(format!("cassie-query-text-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config)
            .expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        Self {
            cassie: Some(cassie),
            session,
            path,
        }
    }
    pub(super) fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live fixture")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        std::fs::remove_dir_all(&self.path).expect("remove retired store");
    }
}
pub(super) fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
