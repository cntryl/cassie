//! Exact reads of the mapped tables: dangling edges, shared labels and label alternatives.

use crate::support_sql_fixture::sql_fixture;
use crate::world::{ints, link, node, World};
use cassie::types::Value;

const ENTITY_SETUP: [&str; 9] = [
    "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
    "CREATE TABLE companies (id BIGINT PRIMARY KEY, name TEXT)",
    "CREATE TABLE knows (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL)",
    "CREATE TABLE works (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL)",
    "INSERT INTO people VALUES (1, 'ann'), (2, 'bob')",
    "INSERT INTO companies VALUES (10, 'acme')",
    "INSERT INTO knows VALUES (100, 1, 2)",
    "INSERT INTO works VALUES (200, 1, 10), (201, 2, 10)",
    "CREATE PROPERTY GRAPH org VERTEX TABLES (people AS person KEY (id) LABEL entity PROPERTIES (id, name), \
     companies AS company KEY (id) LABEL entity PROPERTIES (id, name)) \
     EDGE TABLES (knows AS knows_edge KEY (id) SOURCE KEY (a) REFERENCES person (id) \
     DESTINATION KEY (b) REFERENCES person (id) LABEL rel PROPERTIES (id), \
     works AS works_edge KEY (id) SOURCE KEY (a) REFERENCES person (id) \
     DESTINATION KEY (b) REFERENCES company (id) LABEL rel PROPERTIES (id))",
];

fn count(world: &World, sql: &str) -> usize {
    world.fixture.rows(sql).len()
}

#[test]
pub(crate) fn should_exclude_an_edge_whose_endpoint_vertex_is_missing() {
    // Arrange
    let world = World::simple(&[node(1), node(2)], &[link(1, 1, 2), link(2, 1, 9)]);
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[e IS link]-> (t IS node) \
               COLUMNS (e.num AS e)) AS t";

    // Act
    let before = ints(world.fixture.rows(sql));

    // Assert
    assert_eq!(before, vec![vec![1]]);
}

#[test]
pub(crate) fn should_show_a_dangling_edge_once_its_endpoint_is_inserted() {
    // Arrange
    let world = World::simple(&[node(1), node(2)], &[link(1, 1, 2), link(2, 1, 9)]);
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[e IS link]-> (t IS node) \
               COLUMNS (e.num AS e)) AS t";

    // Act
    let before = count(&world, sql);
    world
        .fixture
        .execute("INSERT INTO node VALUES (9, 'late', 9)")
        .expect("insert endpoint");
    let after = count(&world, sql);

    // Assert
    assert_eq!((before, after), (1, 2));
}

#[test]
pub(crate) fn should_select_several_mappings_that_share_a_label() {
    // Arrange
    let fixture = sql_fixture("graph-shared-label", &ENTITY_SETUP);

    // Act
    let entities = fixture
        .rows("SELECT * FROM GRAPH_TABLE (org MATCH (a IS entity) COLUMNS (a.name AS name)) AS t");
    let relations = fixture.rows(
        "SELECT * FROM GRAPH_TABLE (org MATCH (a IS entity) -[e IS rel]-> (b IS entity) \
         COLUMNS (e.id AS id)) AS t ORDER BY id",
    );

    // Assert
    assert_eq!(entities.len(), 3);
    assert_eq!(ints(relations), vec![vec![100], vec![200], vec![201]]);
}

#[test]
pub(crate) fn should_not_cross_endpoint_mappings_of_an_edge() {
    // Arrange
    let fixture = sql_fixture("graph-endpoint-mapping", &ENTITY_SETUP);

    // Act
    let rows = fixture.rows(
        "SELECT * FROM GRAPH_TABLE (org MATCH (a IS entity) -[e IS rel]-> (b IS entity) \
         COLUMNS (a.name AS source, b.name AS target)) AS t ORDER BY source, target",
    );

    // Assert
    let text = |value: &str| Value::String(value.to_owned());
    assert_eq!(
        rows,
        vec![
            vec![text("ann"), text("acme")],
            vec![text("ann"), text("bob")],
            vec![text("bob"), text("acme")],
        ]
    );
}

#[test]
pub(crate) fn should_count_a_mapping_carrying_both_listed_labels_once() {
    // Arrange
    let fixture = sql_fixture(
        "graph-label-alternatives",
        &[
            "CREATE TABLE things (id BIGINT PRIMARY KEY)",
            "CREATE TABLE ties (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL)",
            "INSERT INTO things VALUES (1), (2)",
            "CREATE PROPERTY GRAPH t VERTEX TABLES (things AS thing KEY (id) \
             LABEL alpha PROPERTIES (id) LABEL beta PROPERTIES (id)) \
             EDGE TABLES (ties AS tie KEY (id) SOURCE KEY (a) REFERENCES thing (id) \
             DESTINATION KEY (b) REFERENCES thing (id) LABEL tie PROPERTIES (id))",
        ],
    );

    // Act
    let both = fixture
        .rows("SELECT * FROM GRAPH_TABLE (t MATCH (a IS alpha | beta) COLUMNS (a.id AS id)) AS g");
    let single =
        fixture.rows("SELECT * FROM GRAPH_TABLE (t MATCH (a IS alpha) COLUMNS (a.id AS id)) AS g");

    // Assert
    assert_eq!((both.len(), single.len()), (2, 2));
}

#[test]
pub(crate) fn should_leave_out_an_edge_with_a_null_endpoint_component() {
    // Arrange
    let fixture = sql_fixture(
        "graph-null-endpoint",
        &[
            "CREATE TABLE v (id BIGINT PRIMARY KEY)",
            "CREATE TABLE ed (id BIGINT PRIMARY KEY, a BIGINT, b BIGINT)",
            "INSERT INTO v VALUES (1), (2)",
            "INSERT INTO ed VALUES (10, 1, 2), (11, 1, NULL), (12, NULL, 2)",
            "CREATE PROPERTY GRAPH nulls VERTEX TABLES (v AS vv KEY (id) LABEL vl PROPERTIES (id)) \
             EDGE TABLES (ed AS ee KEY (id) SOURCE KEY (a) REFERENCES vv (id) \
             DESTINATION KEY (b) REFERENCES vv (id) LABEL el PROPERTIES (id))",
        ],
    );

    // Act
    let rows = fixture.rows(
        "SELECT * FROM GRAPH_TABLE (nulls MATCH (a IS vl) -[e IS el]-> (b IS vl) \
         COLUMNS (e.id AS id)) AS g",
    );

    // Assert
    assert_eq!(ints(rows), vec![vec![10]]);
}
