//! A local-storage Cassie instance seeded by SQL statements, for tests that
//! only need to run queries and inspect rows or errors.
#![allow(dead_code)]

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;

pub struct SqlFixture {
    pub cassie: Cassie,
    pub session: CassieSession,
    path: String,
}

/// Opens a fresh local-storage instance and runs every `setup` statement.
///
/// # Panics
///
/// Panics when the instance cannot be created or a setup statement fails.
pub fn sql_fixture(label: &str, setup: &[&str]) -> SqlFixture {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        assert!(
            cassie.execute_sql(&session, sql, vec![]).is_ok(),
            "setup statement failed: {sql}"
        );
    }
    SqlFixture {
        cassie,
        session,
        path,
    }
}

impl SqlFixture {
    /// Runs `sql` and returns the full result.
    ///
    /// # Errors
    ///
    /// Returns the engine error for a failing statement.
    pub fn execute(&self, sql: &str) -> Result<QueryResult, CassieError> {
        self.cassie.execute_sql(&self.session, sql, vec![])
    }

    /// Runs `sql` and returns its rows.
    ///
    /// # Panics
    ///
    /// Panics when the statement fails.
    pub fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        let Ok(result) = self.execute(sql) else {
            panic!("query failed: {sql}");
        };
        result.rows
    }

    /// Runs `sql`, which must fail, and returns the error.
    ///
    /// # Panics
    ///
    /// Panics when the statement succeeds.
    pub fn error(&self, sql: &str) -> CassieError {
        match self.execute(sql) {
            Ok(_) => panic!("query should fail: {sql}"),
            Err(error) => error,
        }
    }
}

impl Drop for SqlFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
