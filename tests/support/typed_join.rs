//! Isolated configured joins for typed-versus-existing execution comparisons.
use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::executor::QueryResult;

pub struct JoinFixture {
    pub cassie: Cassie,
    session: CassieSession,
    path: String,
}

impl JoinFixture {
    /// Opens an isolated configured engine and runs the fixture statements.
    ///
    /// # Panics
    ///
    /// Panics when engine construction or a setup statement fails.
    pub fn new(native: bool, setup: &[&str]) -> Self {
        crate::support_sql::use_local_storage();
        let path = crate::support_sql::data_dir("typed-join-differential");
        let mut config = CassieRuntimeConfig::default();
        config.limits.vectorized_joins_enabled = native;
        config.limits.vectorized_join_batch_size = 2;
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured join engine");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("join setup");
        }
        Self {
            cassie,
            session,
            path,
        }
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

impl Drop for JoinFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
