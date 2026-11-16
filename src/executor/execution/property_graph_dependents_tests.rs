//! Lifecycle laws of tables mapped by property graphs (#1255).

use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::PropertyGraphMeta;
use crate::config::CassieRuntimeConfig;

const SOCIAL: &str = "CREATE PROPERTY GRAPH social
  VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name))
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (cost AS price))";

fn tables(schema: &str) -> [String; 2] {
    [
        format!("CREATE TABLE {schema}people (id BIGINT PRIMARY KEY, name TEXT, nick TEXT)"),
        format!(
            "CREATE TABLE {schema}follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, \
             followee_id BIGINT NOT NULL, cost INT, note TEXT)"
        ),
    ]
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Result<String, CassieError> {
    cassie
        .execute_sql(session, sql, vec![])
        .map(|result| result.command().to_owned())
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in tables("") {
        run(&cassie, &session, &sql).expect("table");
    }
    run(&cassie, &session, SOCIAL).expect("graph");
    (cassie, session)
}

fn state(cassie: &Cassie, session: &CassieSession, sql: &str) -> &'static str {
    run(cassie, session, sql)
        .expect_err("statement should fail")
        .sql_state()
}

fn graph(cassie: &Cassie, name: &str) -> Option<PropertyGraphMeta> {
    cassie
        .catalog
        .get_property_graph(&cassie.midge.canonical_collection_name(name))
}

/// A graph over one table keyed by `code`, whose unique key is `key_ddl`.
fn keyed(table_ddl: &str, extra: &[&str]) -> (Cassie, CassieSession) {
    let (cassie, session) = fixture();
    run(&cassie, &session, table_ddl).expect("table");
    for sql in extra {
        run(&cassie, &session, sql).expect("setup");
    }
    run(
        &cassie,
        &session,
        "CREATE PROPERTY GRAPH coded
           VERTEX TABLES (codes AS code KEY (code) LABEL code PROPERTIES (code))
           EDGE TABLES (links AS link KEY (id)
             SOURCE KEY (a) REFERENCES code (code)
             DESTINATION KEY (b) REFERENCES code (code)
             LABEL link PROPERTIES (id))",
    )
    .expect("graph");
    (cassie, session)
}

const LINKS: &str =
    "CREATE TABLE links (id BIGINT, a TEXT, b TEXT, CONSTRAINT links_pk PRIMARY KEY (id))";

#[test]
fn should_reject_dropping_a_mapped_table_until_its_graph_is_dropped() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let plain = state(&cassie, &session, "DROP TABLE follows");
    let cascade = state(&cassie, &session, "DROP TABLE follows CASCADE");
    let still_mapped = graph(&cassie, "social").is_some();
    run(&cassie, &session, "DROP PROPERTY GRAPH social").expect("drop graph");
    let after = run(&cassie, &session, "DROP TABLE follows");

    // Assert
    assert_eq!(plain, "2BP01");
    assert_eq!(cascade, "0A000");
    assert!(still_mapped);
    assert!(after.is_ok());
}

#[test]
fn should_reject_dropping_a_column_a_mapping_reads() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let endpoint = state(
        &cassie,
        &session,
        "ALTER TABLE follows DROP COLUMN follower_id",
    );
    let property = state(&cassie, &session, "ALTER TABLE people DROP COLUMN name");
    let renamed_property = state(&cassie, &session, "ALTER TABLE follows DROP COLUMN cost");
    let cascade = state(
        &cassie,
        &session,
        "ALTER TABLE follows DROP COLUMN cost CASCADE",
    );
    let unread = run(&cassie, &session, "ALTER TABLE follows DROP COLUMN note");

    // Assert
    assert_eq!(endpoint, "2BP01");
    assert_eq!(property, "2BP01");
    assert_eq!(renamed_property, "2BP01");
    assert_eq!(cascade, "0A000");
    assert!(unread.is_ok());
}

#[test]
fn should_reject_retyping_a_column_a_mapping_reads() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let endpoint = state(
        &cassie,
        &session,
        "ALTER TABLE follows ALTER COLUMN follower_id TYPE TEXT USING follower_id::text",
    );
    let property = state(
        &cassie,
        &session,
        "ALTER TABLE follows ALTER COLUMN cost TYPE BIGINT USING cost::bigint",
    );
    let unread = run(
        &cassie,
        &session,
        "ALTER TABLE follows ALTER COLUMN note TYPE BIGINT USING note::bigint",
    );

    // Assert
    assert_eq!(endpoint, "2BP01");
    assert_eq!(property, "2BP01");
    assert!(unread.is_ok());
}

#[test]
fn should_reject_making_a_mapped_key_column_nullable() {
    // Arrange
    let (cassie, session) = keyed(
        "CREATE TABLE codes (code TEXT NOT NULL UNIQUE, label TEXT NOT NULL)",
        &[LINKS],
    );

    // Act
    let key = state(
        &cassie,
        &session,
        "ALTER TABLE codes ALTER COLUMN code DROP NOT NULL",
    );
    let other = run(
        &cassie,
        &session,
        "ALTER TABLE codes ALTER COLUMN label DROP NOT NULL",
    );

    // Assert
    assert_eq!(key, "2BP01");
    assert!(other.is_ok());
}

#[test]
fn should_reject_dropping_the_constraint_that_backs_a_mapped_key() {
    // Arrange
    let (cassie, session) = keyed(
        "CREATE TABLE codes (code TEXT NOT NULL, label TEXT, \
         CONSTRAINT codes_code_key UNIQUE (code), CONSTRAINT codes_label_key UNIQUE (label))",
        &[LINKS],
    );

    // Act
    let backing = state(
        &cassie,
        &session,
        "ALTER TABLE codes DROP CONSTRAINT codes_code_key",
    );
    let unrelated = run(
        &cassie,
        &session,
        "ALTER TABLE codes DROP CONSTRAINT codes_label_key",
    );
    let primary = state(
        &cassie,
        &session,
        "ALTER TABLE links DROP CONSTRAINT links_pk",
    );

    // Assert
    assert_eq!(backing, "2BP01");
    assert!(unrelated.is_ok());
    assert_eq!(primary, "2BP01");
}

#[test]
fn should_reject_dropping_the_index_that_backs_a_mapped_key() {
    // Arrange
    let (cassie, session) = keyed(
        "CREATE TABLE codes (code TEXT NOT NULL, label TEXT)",
        &[
            "CREATE UNIQUE INDEX codes_code ON codes (code)",
            "CREATE UNIQUE INDEX codes_label ON codes (label)",
            LINKS,
        ],
    );

    // Act
    let backing = state(&cassie, &session, "DROP INDEX codes_code");
    let unrelated = run(&cassie, &session, "DROP INDEX codes_label");

    // Assert
    assert_eq!(backing, "2BP01");
    assert!(unrelated.is_ok());
}

#[test]
fn should_allow_dropping_one_of_two_keys_that_back_a_mapping() {
    // Arrange
    let (cassie, session) = keyed(
        "CREATE TABLE codes (code TEXT NOT NULL, CONSTRAINT codes_code_key UNIQUE (code))",
        &[
            "CREATE UNIQUE INDEX codes_code_spare ON codes (code)",
            LINKS,
        ],
    );

    // Act
    let first = run(&cassie, &session, "DROP INDEX codes_code_spare");
    let last = state(
        &cassie,
        &session,
        "ALTER TABLE codes DROP CONSTRAINT codes_code_key",
    );

    // Assert
    assert!(first.is_ok());
    assert_eq!(last, "2BP01");
}

#[test]
fn should_follow_a_table_rename_keeping_identities() {
    // Arrange
    let (cassie, session) = fixture();
    let before = graph(&cassie, "social").expect("graph");

    // Act
    run(&cassie, &session, "ALTER TABLE people RENAME TO persons").expect("rename");
    let after = graph(&cassie, "social").expect("graph");
    let stored = cassie.midge.list_property_graphs().expect("list");

    // Assert
    assert_eq!(
        after.vertices[0].table,
        cassie.midge.canonical_collection_name("persons")
    );
    assert_eq!(after.vertices[0].mapping_id, before.vertices[0].mapping_id);
    assert_eq!(after.graph_id, before.graph_id);
    assert_eq!(after.edges, before.edges);
    assert_eq!(stored, vec![after]);
}

#[test]
fn should_follow_column_renames_without_renaming_exposed_properties() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    for sql in [
        "ALTER TABLE follows RENAME COLUMN follower_id TO src",
        "ALTER TABLE follows RENAME COLUMN cost TO amount",
        "ALTER TABLE people RENAME COLUMN id TO pid",
    ] {
        run(&cassie, &session, sql).expect("rename");
    }
    let after = graph(&cassie, "social").expect("graph");
    let stored = cassie.midge.list_property_graphs().expect("list");
    let edge_property = &after.edges[0].labels[0].properties[0];
    let vertex_property = &after.vertices[0].labels[0].properties[0];

    // Assert
    assert_eq!(after.edges[0].source.columns, vec!["src".to_owned()]);
    assert_eq!(after.edges[0].key[0].name, "id");
    assert_eq!(edge_property.column, "amount");
    assert_eq!(edge_property.name, "price");
    assert_eq!(after.vertices[0].key[0].name, "pid");
    assert_eq!(vertex_property.column, "pid");
    assert_eq!(vertex_property.name, "id");
    assert_eq!(stored, vec![after]);
}

#[test]
fn should_follow_a_table_moved_to_another_schema() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE SCHEMA archive").expect("schema");

    // Act
    run(&cassie, &session, "ALTER TABLE people SET SCHEMA archive").expect("move");
    let after = graph(&cassie, "social").expect("graph");

    // Assert
    assert_eq!(
        after.vertices[0].table,
        cassie.midge.canonical_collection_name("archive.people")
    );
}

fn schema_fixture() -> (Cassie, CassieSession) {
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE SCHEMA net").expect("schema");
    for sql in tables("net.") {
        run(&cassie, &session, &sql).expect("table");
    }
    run(
        &cassie,
        &session,
        "CREATE PROPERTY GRAPH net.inner_graph
           VERTEX TABLES (net.people AS person KEY (id) LABEL person PROPERTIES (id))
           EDGE TABLES (net.follows AS knows KEY (id)
             SOURCE KEY (follower_id) REFERENCES person (id)
             DESTINATION KEY (followee_id) REFERENCES person (id)
             LABEL knows PROPERTIES (cost))",
    )
    .expect("graph");
    (cassie, session)
}

#[test]
fn should_move_graphs_with_a_renamed_schema() {
    // Arrange
    let (cassie, session) = schema_fixture();
    let before = graph(&cassie, "net.inner_graph").expect("graph");

    // Act
    run(&cassie, &session, "ALTER SCHEMA net RENAME TO web").expect("rename");
    let after = graph(&cassie, "web.inner_graph").expect("moved graph");
    let stored = cassie.midge.list_property_graphs().expect("list").len();

    // Assert
    assert!(graph(&cassie, "net.inner_graph").is_none());
    assert_eq!(after.graph_id, before.graph_id);
    assert_eq!(
        after.vertices[0].table,
        cassie.midge.canonical_collection_name("web.people")
    );
    assert_eq!(
        after.edges[0].table,
        cassie.midge.canonical_collection_name("web.follows")
    );
    assert_eq!(stored, 2);
}

#[test]
fn should_drop_a_schema_with_its_graphs_only_by_cascade() {
    // Arrange
    let (cassie, session) = schema_fixture();

    // Act
    let plain = run(&cassie, &session, "DROP SCHEMA net").is_err();
    let kept = graph(&cassie, "net.inner_graph").is_some();
    let cascade = run(&cassie, &session, "DROP SCHEMA net CASCADE");

    // Assert
    assert!(plain);
    assert!(kept);
    assert!(cascade.is_ok());
    assert!(graph(&cassie, "net.inner_graph").is_none());
    assert!(graph(&cassie, "social").is_some());
    assert_eq!(cassie.midge.list_property_graphs().expect("list").len(), 1);
}

#[test]
fn should_reject_a_schema_cascade_that_would_orphan_an_outside_graph() {
    // Arrange
    let (cassie, session) = schema_fixture();
    run(
        &cassie,
        &session,
        "CREATE PROPERTY GRAPH outside
           VERTEX TABLES (net.people AS person KEY (id) LABEL person PROPERTIES (id))
           EDGE TABLES (net.follows AS knows KEY (id)
             SOURCE KEY (follower_id) REFERENCES person (id)
             DESTINATION KEY (followee_id) REFERENCES person (id)
             LABEL knows PROPERTIES (cost))",
    )
    .expect("outside graph");

    // Act
    let cascade = state(&cassie, &session, "DROP SCHEMA net CASCADE");

    // Assert
    assert_eq!(cascade, "0A000");
    assert!(graph(&cassie, "net.inner_graph").is_some());
    assert!(graph(&cassie, "outside").is_some());
}

#[test]
fn should_reject_dropping_a_primary_key_that_keeps_a_mapped_key_not_null() {
    // Arrange
    let (cassie, session) = keyed(
        "CREATE TABLE codes (code TEXT, CONSTRAINT codes_pk PRIMARY KEY (code))",
        &[
            "CREATE UNIQUE INDEX codes_code_spare ON codes (code)",
            LINKS,
        ],
    );

    // Act
    let nullable = state(
        &cassie,
        &session,
        "ALTER TABLE codes DROP CONSTRAINT codes_pk",
    );

    // Assert
    assert_eq!(nullable, "2BP01");
}

#[test]
fn should_guard_mapped_tables_in_an_instance_that_was_not_started() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    let session = cassie.create_session("tester", None);
    for sql in tables("") {
        run(&cassie, &session, &sql).expect("table");
    }
    run(&cassie, &session, SOCIAL).expect("graph");

    // Act
    let drop_table = state(&cassie, &session, "DROP TABLE follows");
    let drop_column = state(
        &cassie,
        &session,
        "ALTER TABLE follows DROP COLUMN follower_id",
    );
    run(&cassie, &session, "ALTER TABLE people RENAME TO persons").expect("rename");
    let renamed = cassie
        .catalog
        .list_property_graphs()
        .first()
        .map(|graph| crate::catalog::local_name(&graph.vertices[0].table));

    // Assert
    assert_eq!(drop_table, "2BP01");
    assert_eq!(drop_column, "2BP01");
    assert_eq!(renamed.as_deref(), Some("persons"));
}
