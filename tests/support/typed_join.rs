//! Isolated configured joins for typed-versus-existing execution comparisons.
use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::executor::QueryResult;

pub struct JoinFixture {
    pub cassie: Cassie,
    session: CassieSession,
}

impl JoinFixture {
    /// Opens an isolated configured engine and runs the fixture statements.
    ///
    /// # Panics
    ///
    /// Panics when engine construction or a setup statement fails.
    pub fn new(native: bool, setup: &[&str]) -> Self {
        let mut config = CassieRuntimeConfig::default();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.vectorized_joins_enabled = native;
        config.limits.vectorized_join_batch_size = 2;
        let cassie = Cassie::new_in_memory_with_config(config).expect("configured join engine");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("join setup");
        }
        Self { cassie, session }
    }

    /// Runs one join witness.
    ///
    /// # Panics
    ///
    /// Panics when the witness query fails.
    pub fn execute(&self, sql: &str) -> QueryResult {
        self.execute_with_params(sql, vec![])
    }

    /// Runs one witness with its original SQL value carriers.
    ///
    /// # Panics
    ///
    /// Panics when the witness query fails.
    pub fn execute_with_params(&self, sql: &str, params: Vec<cassie::types::Value>) -> QueryResult {
        self.cassie
            .execute_sql(&self.session, sql, params)
            .expect("join query")
    }
}
