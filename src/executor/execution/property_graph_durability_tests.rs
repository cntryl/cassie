//! Property graph definitions across transactions, restart and databases (#1255).

use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::PropertyGraphMeta;
use crate::config::CassieRuntimeConfig;

const TABLES: [&str; 2] = [
    "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
    "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, \
     followee_id BIGINT NOT NULL)",
];
const SOCIAL: &str = "CREATE PROPERTY GRAPH social
  VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name))
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (id))";

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Result<String, CassieError> {
    cassie
        .execute_sql(session, sql, vec![])
        .map(|result| result.command().to_owned())
}

fn memory() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in TABLES {
        run(&cassie, &session, sql).expect("table");
    }
    (cassie, session)
}

fn graph(cassie: &Cassie, name: &str) -> Option<PropertyGraphMeta> {
    cassie
        .catalog
        .get_property_graph(&cassie.midge.canonical_collection_name(name))
}

#[test]
fn should_discard_a_graph_created_in_a_rolled_back_transaction() {
    // Arrange
    let (cassie, session) = memory();
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, SOCIAL).expect("create");

    // Act
    run(&cassie, &session, "ROLLBACK").expect("rollback");
    let stored = cassie.midge.list_property_graphs().expect("list");
    let recreate = run(&cassie, &session, SOCIAL);

    // Assert
    assert!(stored.is_empty());
    assert!(recreate.is_ok());
}

#[test]
fn should_keep_a_graph_created_in_a_committed_transaction() {
    // Arrange
    let (cassie, session) = memory();
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, SOCIAL).expect("create");

    // Act
    run(&cassie, &session, "COMMIT").expect("commit");
    let other = cassie.create_session("reader", None);
    let duplicate = run(&cassie, &other, SOCIAL)
        .expect_err("already exists")
        .sql_state();

    // Assert
    assert!(graph(&cassie, "social").is_some());
    assert_eq!(cassie.midge.list_property_graphs().expect("list").len(), 1);
    assert_eq!(duplicate, "42710");
}

#[test]
fn should_restore_a_dropped_graph_when_a_savepoint_is_rolled_back() {
    // Arrange
    let (cassie, session) = memory();
    run(&cassie, &session, SOCIAL).expect("create");
    let before = graph(&cassie, "social").expect("graph");
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, "SAVEPOINT keep").expect("savepoint");
    run(&cassie, &session, "DROP PROPERTY GRAPH social").expect("drop");

    // Act
    run(&cassie, &session, "ROLLBACK TO SAVEPOINT keep").expect("rollback to");
    run(&cassie, &session, "COMMIT").expect("commit");
    let after = graph(&cassie, "social");
    let stored = cassie.midge.list_property_graphs().expect("list");

    // Assert
    assert_eq!(after, Some(before.clone()));
    assert_eq!(stored, vec![before]);
}

#[test]
fn should_reload_definitions_with_their_identities_after_restart() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-property-graph-{}", uuid::Uuid::new_v4()));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    for sql in TABLES {
        run(&cassie, &session, sql).expect("table");
    }
    run(&cassie, &session, SOCIAL).expect("create");
    run(
        &cassie,
        &session,
        "ALTER PROPERTY GRAPH social RENAME TO network",
    )
    .expect("rename");
    let before = graph(&cassie, "network").expect("graph");
    cassie.shutdown();
    drop(session);
    drop(cassie);

    // Act
    let recovered =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    recovered.startup().unwrap();
    let after = graph(&recovered, "network");
    let old_name = graph(&recovered, "social");
    let session = recovered.create_session("tester", None);
    let guarded = run(&recovered, &session, "DROP TABLE people")
        .expect_err("still mapped")
        .sql_state();
    recovered.shutdown();
    drop(session);
    drop(recovered);
    std::fs::remove_dir_all(path).unwrap();

    // Assert
    assert_eq!(after, Some(before));
    assert!(old_name.is_none());
    assert_eq!(guarded, "2BP01");
}

#[test]
fn should_scope_graphs_to_their_database() {
    // Arrange
    let (cassie, session) = memory();
    run(&cassie, &session, SOCIAL).expect("create");
    run(&cassie, &session, "CREATE DATABASE other").expect("database");
    let other = cassie.create_session("tester", Some("other".to_owned()));
    for sql in TABLES {
        run(&cassie, &other, sql).expect("table");
    }

    // Act
    let missing = run(&cassie, &other, "DROP PROPERTY GRAPH social")
        .expect_err("not in this database")
        .sql_state();
    run(&cassie, &other, SOCIAL).expect("same name in another database");
    let cross = run(
        &cassie,
        &other,
        "CREATE PROPERTY GRAPH crossing
           VERTEX TABLES (postgres.public.people AS person KEY (id) LABEL person PROPERTIES (id))
           EDGE TABLES (follows AS knows KEY (id)
             SOURCE KEY (follower_id) REFERENCES person (id)
             DESTINATION KEY (followee_id) REFERENCES person (id)
             LABEL knows PROPERTIES (id))",
    )
    .expect_err("cross-database source")
    .sql_state();
    let graphs = cassie.midge.list_property_graphs().expect("list");
    let mut databases = graphs
        .iter()
        .map(|graph| graph.database_id.clone())
        .collect::<Vec<_>>();
    databases.dedup();

    // Assert
    assert_eq!(missing, "42704");
    assert_eq!(cross, "0A000");
    assert_eq!(graphs.len(), 2);
    assert_eq!(databases.len(), 2);
}
