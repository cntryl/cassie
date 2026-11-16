//! A property graph belongs to one database: its reads, its name and its lifecycle stay there.

use crate::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const CREATE_GRAPH: &str = "CREATE PROPERTY GRAPH social \
     VERTEX TABLES (person AS p KEY (id) LABEL person PROPERTIES (id)) \
     EDGE TABLES (knows AS k KEY (id) SOURCE KEY (a) REFERENCES p (id) \
     DESTINATION KEY (b) REFERENCES p (id) LABEL knows PROPERTIES (id))";

const TARGETS: &str = "SELECT * FROM GRAPH_TABLE (social MATCH (s IS person WHERE s.id = 1) \
     -[IS knows]-> (t IS person) COLUMNS (t.id AS id)) AS g ORDER BY id";

fn exec(cassie: &Cassie, session: &CassieSession, sql: &str) {
    if let Err(error) = cassie.execute_sql(session, sql, vec![]) {
        panic!("statement failed with SQLSTATE {}", error.sql_state());
    }
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

/// Creates the databases and returns one session per database.
fn sessions(cassie: &Cassie, databases: &[&str]) -> Vec<CassieSession> {
    let admin = cassie.create_session("tester", None);
    databases
        .iter()
        .map(|database| {
            exec(cassie, &admin, &format!("CREATE DATABASE {database}"));
            cassie.create_session("tester", Some((*database).to_string()))
        })
        .collect()
}

/// Seeds the tables and the graph in the session's database: person 1 knows `target`.
fn seed(cassie: &Cassie, session: &CassieSession, target: i64) {
    exec(
        cassie,
        session,
        "CREATE TABLE person (id BIGINT PRIMARY KEY)",
    );
    exec(
        cassie,
        session,
        "CREATE TABLE knows (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL)",
    );
    exec(
        cassie,
        session,
        &format!("INSERT INTO person VALUES (1), ({target})"),
    );
    exec(
        cassie,
        session,
        &format!("INSERT INTO knows VALUES (1, 1, {target})"),
    );
    exec(cassie, session, CREATE_GRAPH);
}

fn ids(values: &[i64]) -> Vec<Vec<Value>> {
    values.iter().map(|id| vec![Value::Int64(*id)]).collect()
}

#[test]
fn should_read_a_graph_only_in_the_database_that_owns_it() {
    // Arrange
    let fixture = sql_fixture("graph-scope-owner", &[]);
    let cassie = &fixture.cassie;
    let databases = sessions(cassie, &["tenant_a"]);
    let tenant = &databases[0];
    seed(cassie, tenant, 2);

    // Act
    let owner = rows(cassie, tenant, TARGETS);
    let other = cassie.execute_sql(&fixture.session, TARGETS, vec![]);

    // Assert
    assert_eq!(owner, ids(&[2]));
    assert!(
        other.is_err(),
        "another database must not resolve the graph"
    );
}

#[test]
fn should_keep_graphs_of_the_same_name_in_two_databases_apart() {
    // Arrange
    let fixture = sql_fixture("graph-scope-same-name", &[]);
    let cassie = &fixture.cassie;
    let databases = sessions(cassie, &["tenant_a", "tenant_b"]);
    seed(cassie, &databases[0], 2);
    seed(cassie, &databases[1], 3);

    // Act
    let first = rows(cassie, &databases[0], TARGETS);
    let second = rows(cassie, &databases[1], TARGETS);

    // Assert
    assert_eq!((first, second), (ids(&[2]), ids(&[3])));
}

#[test]
fn should_drop_an_empty_database_while_a_graph_exists_in_another_database() {
    // Arrange
    let fixture = sql_fixture("graph-scope-drop-empty", &[]);
    let cassie = &fixture.cassie;
    let databases = sessions(cassie, &["tenant_a", "tenant_empty"]);
    seed(cassie, &databases[0], 2);

    // Act
    let dropped = cassie.execute_sql(&fixture.session, "DROP DATABASE tenant_empty", vec![]);

    // Assert
    assert!(
        dropped.is_ok(),
        "a graph in another database must not make an empty database non-empty"
    );
}

#[test]
fn should_follow_a_column_rename_only_in_the_database_that_renames_it() {
    // Arrange
    let fixture = sql_fixture("graph-scope-rename", &[]);
    let cassie = &fixture.cassie;
    let databases = sessions(cassie, &["tenant_a", "tenant_b"]);
    seed(cassie, &databases[0], 2);
    seed(cassie, &databases[1], 3);

    // Act
    exec(
        cassie,
        &databases[0],
        "ALTER TABLE knows RENAME COLUMN a TO source_id",
    );
    let other_drop = cassie.execute_sql(&databases[1], "ALTER TABLE knows DROP COLUMN a", vec![]);
    let renamed = rows(cassie, &databases[0], TARGETS);
    let untouched = rows(cassie, &databases[1], TARGETS);

    // Assert
    assert!(
        other_drop.is_err(),
        "the other database's graph still maps its own column"
    );
    assert_eq!((renamed, untouched), (ids(&[2]), ids(&[3])));
}
