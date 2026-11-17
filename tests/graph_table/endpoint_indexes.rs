//! Endpoint prefixes require complete index membership.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

const TABLES: &[&str] = &[
    "CREATE TABLE v(id INT PRIMARY KEY)",
    "CREATE TABLE e(id INT PRIMARY KEY,s INT,t INT)",
];
const GRAPH: &str = "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) \
    LABEL node PROPERTIES(id)) EDGE TABLES(e AS l KEY(id) SOURCE KEY(s) REFERENCES n(id) \
    DESTINATION KEY(t) REFERENCES n(id) LABEL edge PROPERTIES(id))";

fn ids(values: &[i64]) -> Vec<Vec<Value>> {
    values
        .iter()
        .map(|value| vec![Value::Int64(*value)])
        .collect()
}

#[test]
fn should_use_a_leading_endpoint_index_when_its_trailing_fields_are_nonnull() {
    // Arrange
    let fixture = sql_fixture("graph-defect-prefix-index", TABLES);
    for sql in [
        "INSERT INTO v VALUES(1),(2),(3),(4)",
        "INSERT INTO e VALUES(11,1,2),(12,3,4),(13,4,3)",
        "CREATE INDEX e_s_id ON e(s,id)",
        GRAPH,
    ] {
        fixture.execute(sql).expect("seed indexed graph");
    }
    let query =
        "SELECT dst FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
                 COLUMNS(b.id AS dst)) AS t";
    let before = fixture.cassie.metrics()["read_paths"]["graph_full_rows"]
        .as_u64()
        .expect("counter");

    // Act
    let rows = fixture.rows(query);
    let after = fixture.cassie.metrics()["read_paths"]["graph_full_rows"]
        .as_u64()
        .expect("counter");

    // Assert
    assert_eq!(rows, ids(&[2]));
    assert_eq!(
        after - before,
        0,
        "a complete endpoint prefix reads only its neighborhood"
    );
}

#[test]
fn should_fall_back_when_a_nullable_index_tail_can_omit_matching_edges() {
    // Arrange
    let fixture = sql_fixture(
        "graph-defect-nullable-prefix",
        &[
            "CREATE TABLE v(id INT PRIMARY KEY)",
            "CREATE TABLE e(id INT PRIMARY KEY,s INT,t INT,optional INT)",
            "INSERT INTO v VALUES(1),(2)",
            "INSERT INTO e VALUES(11,1,2,NULL)",
            "CREATE INDEX e_s_optional ON e(s,optional)",
            GRAPH,
        ],
    );

    // Act
    let rows = fixture.rows(
        "SELECT dst FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
                             COLUMNS(b.id AS dst)) AS t",
    );

    // Assert
    assert_eq!(
        rows,
        ids(&[2]),
        "an index that excludes NULL tails cannot prove membership"
    );
}
