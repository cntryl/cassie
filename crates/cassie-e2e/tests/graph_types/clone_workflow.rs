//! The supported definition export and recreation workflow for a database clone (#1354).
//!
//! A clone is a distinct database with its own identities and no graph definitions.
//! `information_schema.property_graphs.property_graph_definition` renders each visible
//! graph as the exact `CREATE PROPERTY GRAPH` statement, naming tables by schema and local
//! name only; running it in the clone defines the graph there with fresh identities.

use crate::fixture::{started, GraphFixture};
use cassie::app::CassieSession;
use cassie::types::graph::{ElementKind, ElementRef, KeyPart};
use cassie::types::Value;

const ROOMS: &str = "CREATE PROPERTY GRAPH rooms VERTEX TABLES (room AS r KEY (tenant, seq) \
     LABEL room PROPERTIES (tenant, seq AS number)) EDGE TABLES (door AS d KEY (id) \
     SOURCE KEY (a_tenant, a_seq) REFERENCES r (tenant, seq) \
     DESTINATION KEY (b_tenant, b_seq) REFERENCES r (tenant, seq) LABEL door PROPERTIES (weight))";

const SOCIAL: &str = "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) \
     LABEL person PROPERTIES (id, name)) EDGE TABLES (follows AS knows KEY (id) \
     SOURCE KEY (follower_id) REFERENCES person (id) DESTINATION KEY (followee_id) \
     REFERENCES person (id) LABEL knows PROPERTIES (id))";

fn source(label: &str) -> GraphFixture {
    started(
        label,
        &[
            "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
            "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL)",
            "INSERT INTO people VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            "INSERT INTO follows VALUES (10, 1, 2), (11, 2, 3)",
            "CREATE TABLE room (tenant TEXT NOT NULL, seq INT NOT NULL, PRIMARY KEY (tenant, seq))",
            "CREATE TABLE door (id UUID PRIMARY KEY, a_tenant TEXT NOT NULL, a_seq INT NOT NULL, \
             b_tenant TEXT NOT NULL, b_seq INT NOT NULL, weight NUMERIC)",
            "INSERT INTO room VALUES ('t1', 1), ('t1', 2)",
            "INSERT INTO door VALUES ('00000000-0000-0000-0000-0000000000aa', 't1', 1, 't1', 2, 2.5)",
            SOCIAL,
            ROOMS,
        ],
    )
}

fn image_of(fixture: &GraphFixture, database: &str) -> Vec<u8> {
    let mut backup = fixture
        .cassie
        .begin_database_backup(database)
        .expect("backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("stream") {
        image.extend(chunk);
    }
    image
}

/// A clone of the source database named `name`, restored from one image.
fn clone_named(fixture: &GraphFixture, image: &[u8], name: &str) -> CassieSession {
    let mut restore = fixture
        .cassie
        .begin_database_restore(name)
        .expect("begin restore");
    restore.push_chunk(image).expect("push");
    restore.finish().expect("finish");
    fixture
        .cassie
        .create_session("tester", Some(name.to_owned()))
}

fn rows_in(fixture: &GraphFixture, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match fixture.cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("query failed with SQLSTATE {}", error.sql_state()),
    }
}

fn state_in(fixture: &GraphFixture, session: &CassieSession, sql: &str) -> String {
    match fixture.cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => "00000".to_owned(),
        Err(error) => error.sql_state().to_owned(),
    }
}

const EXPORT: &str = "SELECT property_graph_name, property_graph_definition \
     FROM information_schema.property_graphs ORDER BY property_graph_name";

/// The exported `(name, definition)` pairs of `session`'s database, in name order.
fn export(fixture: &GraphFixture, session: &CassieSession) -> Vec<(String, String)> {
    rows_in(fixture, session, EXPORT)
        .into_iter()
        .map(|row| match (&row[0], &row[1]) {
            (Value::String(name), Value::String(definition)) => (name.clone(), definition.clone()),
            _ => panic!("expected text columns"),
        })
        .collect()
}

fn recreate(fixture: &GraphFixture, session: &CassieSession, definitions: &[(String, String)]) {
    for (_, definition) in definitions {
        assert_eq!(state_in(fixture, session, definition), "00000");
    }
}

fn identities(fixture: &GraphFixture, session: &CassieSession) -> Vec<Vec<Value>> {
    rows_in(
        fixture,
        session,
        "SELECT property_graph_name, property_graph_id, database_id \
         FROM pg_catalog.pg_property_graph_identities \
         ORDER BY property_graph_name, element_table_alias",
    )
}

#[test]
fn should_export_the_exact_definition_of_a_graph() {
    // Arrange
    let fixture = source("graph_clone_export_exact");

    // Act
    let exported = export(&fixture, &fixture.session);

    // Assert
    assert_eq!(
        exported,
        vec![
            (
                "rooms".to_owned(),
                "CREATE PROPERTY GRAPH public.rooms VERTEX TABLES (public.room AS r KEY \
                 (tenant, seq) LABEL room PROPERTIES (tenant, seq AS number)) EDGE TABLES \
                 (public.door AS d KEY (id) SOURCE KEY (a_tenant, a_seq) REFERENCES r \
                 (tenant, seq) DESTINATION KEY (b_tenant, b_seq) REFERENCES r (tenant, seq) \
                 LABEL door PROPERTIES (weight))"
                    .to_owned()
            ),
            (
                "social".to_owned(),
                "CREATE PROPERTY GRAPH public.social VERTEX TABLES (public.people AS person \
                 KEY (id) LABEL person PROPERTIES (id, name)) EDGE TABLES (public.follows AS \
                 knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) DESTINATION \
                 KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))"
                    .to_owned()
            ),
        ]
    );
}

#[test]
fn should_recreate_both_exported_graphs_in_a_clone() {
    // Arrange
    let fixture = source("graph_clone_recreate_both");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_both");

    // Act
    recreate(&fixture, &clone, &definitions);

    // Assert
    assert_eq!(export(&fixture, &clone), definitions);
}

#[test]
fn should_give_the_recreated_graphs_identities_of_their_own() {
    // Arrange
    let fixture = source("graph_clone_fresh_identities");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_fresh");

    // Act
    recreate(&fixture, &clone, &definitions);

    // Assert
    let (source_rows, clone_rows) = (
        identities(&fixture, &fixture.session),
        identities(&fixture, &clone),
    );
    assert_eq!(source_rows.len(), clone_rows.len());
    for (before, after) in source_rows.iter().zip(&clone_rows) {
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
        assert_ne!(before[2], after[2]);
    }
}

#[test]
fn should_read_the_same_edges_in_the_clone_as_its_restored_table_rows() {
    // Arrange
    let fixture = source("graph_clone_edges");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_edges");
    recreate(&fixture, &clone, &definitions);

    // Act
    let through_graph = rows_in(
        &fixture,
        &clone,
        "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) -[IS knows]-> (b IS person) \
         COLUMNS (a.id AS a, b.id AS b)) AS t ORDER BY a, b",
    );
    let from_table = rows_in(
        &fixture,
        &clone,
        "SELECT follower_id, followee_id FROM follows ORDER BY follower_id, followee_id",
    );

    // Assert
    assert_eq!(through_graph, from_table);
}

#[test]
fn should_keep_the_graph_unchanged_given_a_rerun_of_the_export() {
    // Arrange
    let fixture = source("graph_clone_rerun");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_rerun");
    recreate(&fixture, &clone, &definitions);
    let before = identities(&fixture, &clone);

    // Act
    let outcome = state_in(&fixture, &clone, &definitions[1].1);

    // Assert
    assert_eq!(outcome, "42710");
    assert_eq!(identities(&fixture, &clone), before);
}

#[test]
fn should_not_publish_a_graph_when_a_mapped_table_is_missing_in_the_clone() {
    // Arrange
    let fixture = source("graph_clone_missing_table");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_missing");
    assert_eq!(state_in(&fixture, &clone, "DROP TABLE follows"), "00000");

    // Act
    let outcome = state_in(&fixture, &clone, &definitions[1].1);

    // Assert
    assert_eq!(outcome, "42P01");
    assert_eq!(export(&fixture, &clone), Vec::<(String, String)>::new());
}

#[test]
fn should_reject_a_graph_query_when_the_failed_recreation_published_nothing() {
    // Arrange
    let fixture = source("graph_clone_missing_query");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(
        &fixture,
        &image_of(&fixture, "postgres"),
        "clone_missing_query",
    );
    assert_eq!(state_in(&fixture, &clone, "DROP TABLE follows"), "00000");
    let _ = state_in(&fixture, &clone, &definitions[1].1);

    // Act
    let outcome = state_in(
        &fixture,
        &clone,
        "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id AS a)) AS t",
    );

    // Assert
    assert_eq!(outcome, "42704");
}

#[test]
fn should_complete_the_recreation_after_the_missing_table_is_restored() {
    // Arrange
    let fixture = source("graph_clone_retry");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_retry");
    assert_eq!(state_in(&fixture, &clone, "DROP TABLE follows"), "00000");
    assert_eq!(state_in(&fixture, &clone, &definitions[1].1), "42P01");
    let create = "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, \
                  followee_id BIGINT NOT NULL)";
    assert_eq!(state_in(&fixture, &clone, create), "00000");

    // Act
    let outcome = state_in(&fixture, &clone, &definitions[1].1);

    // Assert
    assert_eq!(outcome, "00000");
}

fn reference_in(
    fixture: &GraphFixture,
    session: &CassieSession,
    database_rows: &str,
    key: i64,
) -> ElementRef {
    let rows = rows_in(fixture, session, database_rows);
    let uuid = |value: &Value| match value {
        Value::String(text) => uuid::Uuid::parse_str(text).expect("uuid"),
        _ => panic!("expected uuid text"),
    };
    ElementRef {
        database_id: uuid(&rows[0][0]),
        graph_id: uuid(&rows[0][1]),
        mapping_id: uuid(&rows[0][2]),
        kind: ElementKind::Vertex,
        key: vec![KeyPart::BigInt(key)],
    }
}

const MINT: &str = "SELECT (r).database_id, (r).graph_id, (r).mapping_id FROM (SELECT \
     cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part(2::bigint)]) AS r) t";

#[test]
fn should_reject_a_copied_source_reference_in_a_clone_with_like_named_mappings() {
    // Arrange
    let fixture = source("graph_clone_copied_reference");
    let definitions = export(&fixture, &fixture.session);
    let copied = reference_in(&fixture, &fixture.session, MINT, 2);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_copied");
    recreate(&fixture, &clone, &definitions);

    // Act
    let outcome = fixture
        .cassie
        .catalog
        .resolve_element_ref(&copied, "clone_copied");

    // Assert
    assert_eq!(outcome.expect_err("copied reference").sql_state(), "42704");
}

#[test]
fn should_resolve_a_fresh_clone_reference_from_an_explicit_typed_key() {
    // Arrange
    let fixture = source("graph_clone_fresh_reference");
    let definitions = export(&fixture, &fixture.session);
    let clone = clone_named(&fixture, &image_of(&fixture, "postgres"), "clone_fresh_ref");
    recreate(&fixture, &clone, &definitions);

    // Act
    let minted = reference_in(&fixture, &clone, MINT, 2);
    let outcome = fixture
        .cassie
        .catalog
        .resolve_element_ref(&minted, "clone_fresh_ref");

    // Assert
    assert!(outcome.is_ok());
}

#[test]
fn should_reject_a_reference_minted_in_one_clone_in_a_sibling_clone_of_the_same_image() {
    // Arrange
    let fixture = source("graph_clone_siblings");
    let definitions = export(&fixture, &fixture.session);
    let image = image_of(&fixture, "postgres");
    let first = clone_named(&fixture, &image, "clone_first");
    let second = clone_named(&fixture, &image, "clone_second");
    recreate(&fixture, &first, &definitions);
    recreate(&fixture, &second, &definitions);

    // Act
    let from_first = reference_in(&fixture, &first, MINT, 2);
    let outcome = fixture
        .cassie
        .catalog
        .resolve_element_ref(&from_first, "clone_second");

    // Assert
    assert_eq!(outcome.expect_err("sibling reference").sql_state(), "42704");
}

#[test]
fn should_resolve_each_sibling_clones_own_fresh_reference() {
    // Arrange
    let fixture = source("graph_clone_siblings_fresh");
    let definitions = export(&fixture, &fixture.session);
    let image = image_of(&fixture, "postgres");
    let first = clone_named(&fixture, &image, "clone_one");
    let second = clone_named(&fixture, &image, "clone_two");
    recreate(&fixture, &first, &definitions);
    recreate(&fixture, &second, &definitions);

    // Act
    let in_first = reference_in(&fixture, &first, MINT, 2);
    let in_second = reference_in(&fixture, &second, MINT, 2);

    // Assert
    assert!(fixture
        .cassie
        .catalog
        .resolve_element_ref(&in_first, "clone_one")
        .is_ok());
    assert!(fixture
        .cassie
        .catalog
        .resolve_element_ref(&in_second, "clone_two")
        .is_ok());
}

fn restricted_reader(fixture: &GraphFixture, database: &str) -> CassieSession {
    let password = crate::support_sql::test_root_password();
    let root = fixture.cassie.create_session("root", None);
    for sql in [
        format!("CREATE ROLE reader LOGIN PASSWORD '{password}'"),
        "GRANT USAGE ON SCHEMA public TO reader".to_owned(),
        "GRANT SELECT ON TABLE public.people TO reader".to_owned(),
    ] {
        assert_eq!(state_in(fixture, &root, &sql), "00000");
    }
    fixture
        .cassie
        .authenticate_role("reader", Some(password), Some(database.to_owned()))
        .expect("role login")
}

#[test]
fn should_export_no_definition_to_a_reader_who_may_not_read_a_mapped_table() {
    // Arrange
    let fixture = source("graph_clone_unauthorized_export");
    let reader = restricted_reader(&fixture, "postgres");

    // Act
    let exported = export(&fixture, &reader);

    // Assert
    assert_eq!(exported, Vec::<(String, String)>::new());
}

#[test]
fn should_not_publish_a_graph_when_the_recreating_role_may_not_read_a_mapped_table() {
    // Arrange
    let fixture = source("graph_clone_unauthorized_create");
    let definitions = export(&fixture, &fixture.session);
    let reader = restricted_reader(&fixture, "postgres");

    // Act
    let outcome = state_in(&fixture, &reader, &definitions[1].1);

    // Assert
    assert_eq!(outcome, "42501");
    assert_eq!(export(&fixture, &fixture.session).len(), 2);
}

#[test]
fn should_round_trip_a_definition_that_needs_identifier_quoting() {
    // Arrange
    let fixture = started(
        "graph_clone_quoted",
        &[
            "CREATE TABLE \"Cities\" (\"Id\" BIGINT PRIMARY KEY, \"user\" TEXT)",
            "CREATE TABLE \"Roads\" (id BIGINT PRIMARY KEY, \"From\" BIGINT NOT NULL, \"To\" BIGINT NOT NULL)",
            "CREATE PROPERTY GRAPH \"Atlas\" VERTEX TABLES (\"Cities\" AS \"City\" KEY (\"Id\") \
             LABEL \"Place\" PROPERTIES (\"Id\", \"user\" AS \"order\")) EDGE TABLES (\"Roads\" AS \
             \"Road\" KEY (id) SOURCE KEY (\"From\") REFERENCES \"City\" (\"Id\") DESTINATION KEY \
             (\"To\") REFERENCES \"City\" (\"Id\") LABEL \"Link\" PROPERTIES (id))",
        ],
    );
    let exported = export(&fixture, &fixture.session);
    assert_eq!(
        state_in(&fixture, &fixture.session, "DROP PROPERTY GRAPH \"Atlas\""),
        "00000"
    );

    // Act
    recreate(&fixture, &fixture.session, &exported);

    // Assert
    assert_eq!(export(&fixture, &fixture.session), exported);
}
