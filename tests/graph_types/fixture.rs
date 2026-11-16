//! A started in-memory instance: the reserved schema bootstraps at startup.

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;

pub struct GraphFixture {
    pub session: CassieSession,
    pub cassie: Cassie,
}

/// A started instance whose setup statements have run as a plain role.
///
/// # Panics
///
/// Panics when the instance cannot start or a setup statement fails.
#[must_use]
pub fn started(label: &str, setup: &[&str]) -> GraphFixture {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    if config.password.is_empty() {
        crate::support_sql::test_root_password().clone_into(&mut config.password);
    }
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        assert!(
            cassie.execute_sql(&session, sql, vec![]).is_ok(),
            "setup statement failed in {label}: {sql}"
        );
    }
    GraphFixture { session, cassie }
}

impl GraphFixture {
    /// # Errors
    ///
    /// Returns the engine error for a failing statement.
    pub fn execute(&self, sql: &str) -> Result<QueryResult, CassieError> {
        self.cassie.execute_sql(&self.session, sql, vec![])
    }

    /// # Panics
    ///
    /// Panics when the statement fails.
    #[must_use]
    pub fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        match self.execute(sql) {
            Ok(result) => result.rows().to_vec(),
            Err(error) => panic!("query failed: {sql}: {error}"),
        }
    }

    /// The SQLSTATE of a statement that must fail.
    ///
    /// # Panics
    ///
    /// Panics when the statement succeeds.
    #[must_use]
    pub fn sqlstate(&self, sql: &str) -> String {
        match self.execute(sql) {
            Ok(_) => panic!("query should fail: {sql}"),
            Err(error) => error.sql_state().to_owned(),
        }
    }
}
