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

    /// Runs a setup statement that must succeed.
    ///
    /// # Panics
    ///
    /// Panics when the statement fails.
    pub fn run(&self, sql: &str) {
        if let Err(error) = self.execute(sql) {
            panic!("statement failed: {sql}: {error}");
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

/// A table pair and a graph over it, shared by the reference and path suites.
pub const SOCIAL_SETUP: [&str; 5] = [
    "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
    "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL)",
    "INSERT INTO people VALUES (9007199254740993, 'a'), (2, 'b')",
    "INSERT INTO follows VALUES (10, 9007199254740993, 2)",
    "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name)) \
     EDGE TABLES (follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) \
     DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))",
];

/// A vertex reference expression over the shared graph.
#[must_use]
pub fn person(id: i64) -> String {
    format!("cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part({id}::bigint)])")
}

/// An edge reference expression over the shared graph.
#[must_use]
pub fn knows(id: i64) -> String {
    format!("cassie_graph.edge_ref('social', 'knows', ARRAY[cassie_graph.key_part({id}::bigint)])")
}

/// A one-hop path expression from `from` to `to` over edge 10.
#[must_use]
pub fn one_hop(from: i64, to: i64, direction: i16) -> String {
    format!(
        "ROW(ARRAY[{}, {}], ARRAY[{}], ARRAY[{direction}::smallint])::cassie_graph.graph_path",
        person(from),
        person(to),
        knows(10)
    )
}
