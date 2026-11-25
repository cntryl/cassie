//! Requested labels and quoted property identifiers retain their SQL meaning.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

fn ids(values: &[i64]) -> Vec<Vec<Value>> {
    values
        .iter()
        .map(|value| vec![Value::Int64(*value)])
        .collect()
}

#[test]
fn should_resolve_properties_in_the_order_of_requested_pattern_labels() {
    // Arrange
    let fixture = sql_fixture(
        "graph-defect-label-order",
        &[
            "CREATE TABLE v(id INT PRIMARY KEY,a INT,b INT)",
            "CREATE TABLE e(id INT PRIMARY KEY,s INT,t INT)",
            "INSERT INTO v VALUES(1,10,20)",
            "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) \
         LABEL a PROPERTIES(a AS x) LABEL b PROPERTIES(b AS x)) \
         EDGE TABLES(e AS l KEY(id) SOURCE KEY(s) REFERENCES n(id) \
         DESTINATION KEY(t) REFERENCES n(id) LABEL edge PROPERTIES(id))",
        ],
    );
    let query = |labels: &str| {
        format!("SELECT x FROM GRAPH_TABLE(g MATCH(n IS {labels}) COLUMNS(n.x)) AS t")
    };

    // Act
    let ab = fixture.rows(&query("a|b"));
    let ba = fixture.rows(&query("b|a"));
    let b = fixture.rows(&query("b"));

    // Assert
    assert_eq!(ab, ids(&[10]));
    assert_eq!(ba, ids(&[20]));
    assert_eq!(b, ba);
}

#[test]
fn should_distinguish_a_quoted_label_from_its_lowercase_sibling() {
    // Arrange
    let fixture = sql_fixture("graph-defect-quoted-label", &[
        "CREATE TABLE up(id INT PRIMARY KEY)",
        "CREATE TABLE low(id INT PRIMARY KEY)",
        "CREATE TABLE e(id INT PRIMARY KEY,s INT,t INT)",
        "INSERT INTO up VALUES(11)",
        "INSERT INTO low VALUES(22)",
        "CREATE PROPERTY GRAPH g VERTEX TABLES(up AS u KEY(id) LABEL \"Person\" PROPERTIES(id), \
         low AS l KEY(id) LABEL person PROPERTIES(id)) EDGE TABLES(e AS e KEY(id) \
         SOURCE KEY(s) REFERENCES u(id) DESTINATION KEY(t) REFERENCES u(id) LABEL edge PROPERTIES(id))",
    ]);

    // Act
    let upper =
        fixture.rows("SELECT id FROM GRAPH_TABLE(g MATCH(v IS \"Person\") COLUMNS(v.id)) AS t");
    let lower = fixture.rows("SELECT id FROM GRAPH_TABLE(g MATCH(v IS person) COLUMNS(v.id)) AS t");

    // Assert
    assert_eq!(upper, ids(&[11]));
    assert_eq!(lower, ids(&[22]));
}

#[test]
fn should_resolve_a_quoted_key_column_without_selecting_its_lowercase_sibling() {
    // Arrange
    let fixture = sql_fixture(
        "graph-defect-quoted-key",
        &[
            "CREATE TABLE v(id INT PRIMARY KEY,\"ID\" TEXT NOT NULL UNIQUE)",
            "CREATE TABLE e(id INT PRIMARY KEY,s TEXT,t TEXT)",
            "INSERT INTO v VALUES(1,'key-a')",
        ],
    );

    // Act
    let created = fixture.execute("CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(\"ID\") \
        LABEL node PROPERTIES(\"ID\")) EDGE TABLES(e AS l KEY(id) SOURCE KEY(s) REFERENCES n(\"ID\") \
        DESTINATION KEY(t) REFERENCES n(\"ID\") LABEL edge PROPERTIES(id))");

    // Assert
    assert!(
        created.is_ok(),
        "quoted TEXT key must match TEXT endpoints: {created:?}"
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.\"ID\")) AS t"),
        vec![vec![Value::String("key-a".to_owned())]]
    );
}

#[test]
fn should_resolve_a_property_whose_quoted_name_contains_a_dot() {
    // Arrange
    let fixture = sql_fixture("graph-defect-dotted-property", &[
        "CREATE TABLE v(id BIGINT PRIMARY KEY)",
        "CREATE TABLE e(id BIGINT PRIMARY KEY,s BIGINT,t BIGINT)",
        "INSERT INTO v VALUES(2)",
        "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) LABEL node \
         PROPERTIES(id AS \"user.id\",id AS plain)) EDGE TABLES(e AS l KEY(id) \
         SOURCE KEY(s) REFERENCES n(id) DESTINATION KEY(t) REFERENCES n(id) LABEL edge PROPERTIES(id))",
    ]);

    // Act
    let implicit =
        fixture.rows("SELECT * FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.\"user.id\")) AS t");
    let explicit = fixture.rows(
        "SELECT * FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.\"user.id\" AS selected)) AS t",
    );

    // Assert
    assert_eq!(implicit, ids(&[2]));
    assert_eq!(explicit, implicit);
}
