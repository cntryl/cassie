//! Equivalent quantified splits bind singleton elements once.

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
fn should_deduplicate_quantified_splits_that_bind_the_same_singleton_elements() {
    // Arrange
    let fixture = sql_fixture("graph-defect-same-binding", TABLES);
    for sql in [
        "INSERT INTO v VALUES(1)",
        "INSERT INTO e VALUES(1,1,1)",
        GRAPH,
    ] {
        fixture.execute(sql).expect("seed self loop");
    }
    let query = |middle: &str| {
        format!(
            "SELECT jsonb_array_length(cassie_graph.path_to_jsonb(t.path)->'edges') AS hops \
         FROM GRAPH_TABLE(g MATCH p=(a)-[]->{{0,1}}({middle})-[]->{{0,1}}(c) \
         COLUMNS(p AS path)) AS t ORDER BY hops"
        )
    };

    // Act
    let named = fixture.rows(&query("b"));
    let anonymous = fixture.rows(&query(""));

    // Assert
    assert_eq!(named, ids(&[0, 1, 2]));
    assert_eq!(anonymous, named);
}

#[test]
fn should_retain_quantified_splits_that_bind_different_singleton_elements() {
    // Arrange
    let fixture = sql_fixture("graph-defect-distinct-binding", TABLES);
    for sql in [
        "INSERT INTO v VALUES(1),(2)",
        "INSERT INTO e VALUES(1,1,2)",
        GRAPH,
    ] {
        fixture.execute(sql).expect("seed one edge");
    }

    // Act
    let named = fixture.rows("SELECT dst FROM GRAPH_TABLE(g MATCH(a WHERE a.id=1)-[]->{0,1}(b)-[]->{0,1}(c WHERE c.id=2) \
        COLUMNS(c.id AS dst)) AS t");
    let anonymous = fixture.rows("SELECT dst FROM GRAPH_TABLE(g MATCH(a WHERE a.id=1)-[]->{0,1}()-[]->{0,1}(c WHERE c.id=2) \
        COLUMNS(c.id AS dst)) AS t");

    // Assert
    assert_eq!(named, ids(&[2, 2]));
    assert_eq!(anonymous, ids(&[2]));
}
