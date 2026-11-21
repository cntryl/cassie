//! A recreated clone graph survives reopening its local storage (#1354): the exported
//! definition, the identities of the clone's graph and its query result are unchanged.

use crate::support_sql::data_dir;
use cassie::app::Cassie;
use cassie::types::Value;

const SETUP: [&str; 5] = [
    "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
    "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL)",
    "INSERT INTO people VALUES (1, 'a'), (2, 'b')",
    "INSERT INTO follows VALUES (10, 1, 2)",
    "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) LABEL person \
     PROPERTIES (id, name)) EDGE TABLES (follows AS knows KEY (id) SOURCE KEY (follower_id) \
     REFERENCES person (id) DESTINATION KEY (followee_id) REFERENCES person (id) \
     LABEL knows PROPERTIES (id))",
];

const CLONE: &str = "clone_reopen";
const EXPORT: &str = "SELECT property_graph_definition FROM information_schema.property_graphs";
const IDENTITY: &str = "SELECT property_graph_id, database_id \
     FROM pg_catalog.pg_property_graph_identities ORDER BY element_table_alias";
const EDGES: &str = "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) -[IS knows]-> \
     (b IS person) COLUMNS (a.id AS a, b.id AS b)) AS t";

fn open(path: &str) -> Cassie {
    let cassie = Cassie::new_with_data_dir(path).expect("open storage");
    cassie.startup().expect("start up");
    cassie
}

fn rows(cassie: &Cassie, database: Option<&str>, sql: &str) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", database.map(str::to_owned));
    match cassie.execute_sql(&session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("query failed with SQLSTATE {}", error.sql_state()),
    }
}

/// Builds the source, clones it and recreates the exported graph in the clone.
fn first_run(path: &str) -> String {
    let cassie = open(path);
    for sql in SETUP {
        let _ = rows(&cassie, None, sql);
    }
    let Value::String(definition) = rows(&cassie, None, EXPORT)[0][0].clone() else {
        panic!("a definition");
    };
    let mut backup = cassie.begin_database_backup("postgres").expect("backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("stream") {
        image.extend(chunk);
    }
    let mut restore = cassie.begin_database_restore(CLONE).expect("restore");
    restore.push_chunk(&image).expect("push");
    restore.finish().expect("finish");
    let _ = rows(&cassie, Some(CLONE), &definition);
    definition
}

#[test]
fn should_keep_the_recreated_clone_graph_after_reopening_the_storage() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = data_dir("graph-clone-restart");
    let definition = first_run(&path);
    let cassie = open(&path);
    let before = (
        rows(&cassie, Some(CLONE), IDENTITY),
        rows(&cassie, Some(CLONE), EDGES),
    );
    drop(cassie);

    // Act
    let reopened = open(&path);
    let after = (
        rows(&reopened, Some(CLONE), IDENTITY),
        rows(&reopened, Some(CLONE), EDGES),
    );
    let exported = rows(&reopened, Some(CLONE), EXPORT);
    drop(reopened);
    let _ = std::fs::remove_dir_all(&path);

    // Assert
    assert_eq!(before, after);
    assert_eq!(exported, vec![vec![Value::String(definition)]]);
}
