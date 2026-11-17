//! View dependencies follow graph identity within the owning database.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE TABLE v(id INT PRIMARY KEY)",
    "CREATE TABLE e(id INT PRIMARY KEY,a INT,b INT)",
    "INSERT INTO v VALUES(1)",
    "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) \
     EDGE TABLES(e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
     DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
];
const VIEW: &str = "CREATE VIEW vg AS SELECT x FROM GRAPH_TABLE(public.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t";

#[test]
fn should_reject_a_graph_rename_while_its_view_depends_on_the_old_name() {
    // Arrange
    let fixture = sql_fixture("graph-view-rename-dependency", SETUP);
    fixture.execute(VIEW).expect("create dependent view");

    // Act
    let renamed = fixture.execute("ALTER PROPERTY GRAPH g RENAME TO renamed");

    // Assert
    assert_eq!(
        renamed
            .expect_err("dependent view blocks rename")
            .sql_state(),
        "2BP01"
    );
    assert_eq!(
        fixture.rows("SELECT x FROM vg"),
        vec![vec![Value::Int64(1)]]
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(1)]]
    );
    assert!(fixture
        .execute("SELECT * FROM GRAPH_TABLE(renamed MATCH(v IS node) COLUMNS(v.id)) AS t")
        .is_err());
}

#[test]
fn should_drop_the_transitive_graph_view_closure_with_its_schema_and_restore_it_on_rollback() {
    // Arrange
    let fixture = sql_fixture("graph-view-schema-cascade", &[
        "CREATE TABLE v(id INT PRIMARY KEY)",
        "CREATE TABLE e(id INT PRIMARY KEY,a INT,b INT)",
        "INSERT INTO v VALUES(1)",
        "CREATE SCHEMA graph_schema",
        "CREATE PROPERTY GRAPH graph_schema.g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) \
         EDGE TABLES(e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
         DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
        "CREATE VIEW vg AS SELECT x FROM GRAPH_TABLE(graph_schema.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t",
        "CREATE VIEW vg_outer AS SELECT x FROM vg",
        "CREATE VIEW unrelated AS SELECT 7 AS x",
    ]);
    fixture.execute("BEGIN").expect("begin cascade transaction");

    // Act
    fixture
        .execute("DROP SCHEMA graph_schema CASCADE")
        .expect("cascade graph schema");
    let surviving = fixture
        .rows("SELECT relname FROM pg_class WHERE relname IN ('vg','vg_outer') ORDER BY relname");
    let unrelated = fixture.rows("SELECT x FROM unrelated");
    fixture
        .execute("ROLLBACK")
        .expect("restore graph and view closure");
    let restored = fixture.rows("SELECT x FROM vg_outer");

    // Assert
    assert!(
        surviving.is_empty(),
        "direct and transitive external views must be removed"
    );
    assert_eq!(unrelated, vec![vec![Value::Int64(7)]]);
    assert_eq!(restored, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_ignore_other_databases_when_checking_graph_view_dependencies() {
    // Arrange
    let fixture = sql_fixture("graph-view-database-dependency", SETUP);
    fixture.execute(VIEW).expect("create dependent view");
    fixture
        .execute("CREATE DATABASE other_graph_db")
        .expect("create independent database");
    let other = fixture
        .cassie
        .create_session("tester", Some("other_graph_db".to_owned()));
    for sql in SETUP {
        fixture
            .cassie
            .execute_sql(&other, sql, vec![])
            .expect("seed independent graph");
    }

    // Act
    let dropped = fixture
        .cassie
        .execute_sql(&other, "DROP PROPERTY GRAPH g", vec![]);
    let original_view = fixture.rows("SELECT x FROM vg");

    // Assert
    assert!(
        dropped.is_ok(),
        "a foreign database view cannot depend on this graph: {dropped:?}"
    );
    assert_eq!(original_view, vec![vec![Value::Int64(1)]]);
}
