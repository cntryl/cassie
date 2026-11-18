//! Graph view dependencies follow the same resolved CTE identity as execution.

use crate::graph_view_fixture::started_shadow_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_quoted_cte_views_during_graph_schema_cascade() {
    // Arrange
    let fixture = started_shadow_fixture("cte-graph-view-identity", &[
        "CREATE SCHEMA common", "CREATE SCHEMA graph_scope", "CREATE SCHEMA kept",
        "CREATE TABLE common.v(id INT PRIMARY KEY)",
        "CREATE TABLE common.e(id INT PRIMARY KEY,a INT,b INT)",
        "INSERT INTO common.v VALUES(2)",
        "CREATE PROPERTY GRAPH graph_scope.g VERTEX TABLES(common.v AS n KEY(id) LABEL node PROPERTIES(id)) EDGE TABLES(common.e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
        "CREATE VIEW public.\"Base\" AS SELECT x FROM GRAPH_TABLE(graph_scope.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t",
        "CREATE VIEW kept.independent AS WITH \"Base\" AS (SELECT 77 AS x) SELECT x FROM \"Base\"",
        "CREATE VIEW kept.nested AS WITH \"Base\" AS (SELECT 88 AS x) SELECT x FROM (SELECT x FROM \"Base\") AS local_rows",
        "CREATE VIEW kept.body_dependency AS WITH \"Base\" AS (SELECT x FROM public.\"Base\") SELECT x FROM \"Base\"",
        "CREATE VIEW kept.unrelated AS SELECT 99 AS x",
    ]);
    assert_eq!(
        fixture.rows("SELECT x FROM public.\"Base\""),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM kept.body_dependency"),
        vec![vec![Value::Int64(2)]]
    );

    // Act
    let local_before = fixture.execute("SELECT x FROM kept.independent");
    fixture
        .execute("DROP SCHEMA graph_scope CASCADE")
        .expect("unique graph closure");
    let local_after = fixture.execute("SELECT x FROM kept.independent");
    let nested_after = fixture.execute("SELECT x FROM kept.nested");
    let body_after = fixture.execute("SELECT x FROM kept.body_dependency");

    // Assert
    assert_eq!(
        local_before.expect("accepted quoted local view").rows(),
        &[vec![Value::Int64(77)]]
    );
    assert_eq!(
        local_after.expect("local CTE view survives").rows(),
        &[vec![Value::Int64(77)]]
    );
    assert_eq!(
        nested_after.expect("nested local CTE view survives").rows(),
        &[vec![Value::Int64(88)]]
    );
    assert_eq!(
        body_after
            .expect_err("real CTE-body catalog dependency cascades")
            .sql_state(),
        "42P01"
    );
    assert_eq!(
        fixture.rows("SELECT x FROM kept.unrelated"),
        vec![vec![Value::Int64(99)]]
    );
}
