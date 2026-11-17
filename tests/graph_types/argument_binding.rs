//! Constructor arguments preserve their bound SQL type and computed expressions.

use crate::fixture::{started, SOCIAL_SETUP};
use cassie::types::Value;

#[test]
fn should_reject_explicit_text_arrays_as_graph_key_parts() {
    // Arrange
    let fixture = started("graph-argument-text-array", &SOCIAL_SETUP);

    // Act
    let rejected = fixture
        .execute("SELECT cassie_graph.vertex_ref('social','person',ARRAY['(3,2,,)']::text[])");
    let accepted = fixture.rows(
        "SELECT k.integer_value FROM cassie_graph.key_parts(\
        cassie_graph.vertex_ref('social','person',ARRAY[cassie_graph.key_part(2::bigint)])) AS k",
    );

    // Assert
    assert_eq!(
        rejected
            .expect_err("text[] is not graph_key_part[]")
            .sql_state(),
        "42804"
    );
    assert_eq!(accepted, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_bind_computed_key_parts_inside_table_function_arguments() {
    // Arrange
    let fixture = started("graph-argument-computed-key", &SOCIAL_SETUP);

    // Act
    let rows = fixture.rows(
        "SELECT k.integer_value FROM cassie_graph.key_parts(\
        cassie_graph.vertex_ref('social','person',\
        ARRAY[cassie_graph.key_part(2::bigint+0::bigint)])) AS k",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_bind_computed_graph_key_parts_inside_another_table_function() {
    // Arrange
    let fixture = started("graph-argument-computed-unnest", &SOCIAL_SETUP);
    let count = |operand: &str| {
        fixture.execute(&format!(
            "SELECT count(*) FROM unnest(ARRAY[cassie_graph.vertex_ref(\
             'social','person',ARRAY[cassie_graph.key_part({operand})])]) AS u"
        ))
    };

    // Act
    let simple = count("2::bigint");
    let explicitly_typed = count("(2::bigint+0::bigint)::bigint");
    let computed = count("2::bigint+0::bigint");

    // Assert
    let expected = vec![vec![Value::Int64(1)]];
    assert_eq!(simple.expect("simple graph reference").rows(), &expected);
    assert_eq!(
        explicitly_typed
            .expect("explicit computed graph reference")
            .rows(),
        &expected
    );
    assert_eq!(
        computed.expect("computed graph reference").rows(),
        &expected
    );
}

#[test]
fn should_resolve_a_mapping_alias_without_folding_a_quoted_sibling() {
    // Arrange
    let fixture = started("graph-argument-quoted-mapping", &[
        "CREATE TABLE vb(id BIGINT PRIMARY KEY)",
        "CREATE TABLE vi(id INT PRIMARY KEY)",
        "CREATE TABLE e(id INT PRIMARY KEY,a INT,b INT)",
        "INSERT INTO vb VALUES(1)",
        "INSERT INTO vi VALUES(1)",
        "CREATE PROPERTY GRAPH g VERTEX TABLES(vb AS \"N\" KEY(id) LABEL upper PROPERTIES(id),\
         vi AS n KEY(id) LABEL lower PROPERTIES(id)) EDGE TABLES(e AS l KEY(id) \
         SOURCE KEY(a) REFERENCES n(id) DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
    ]);

    // Act
    let lower = fixture.rows(
        "SELECT k.integer_value FROM cassie_graph.key_parts(\
        cassie_graph.vertex_ref('g','n',ARRAY[cassie_graph.key_part(1::int)])) AS k",
    );
    let upper = fixture.rows(
        "SELECT k.integer_value FROM cassie_graph.key_parts(\
        cassie_graph.vertex_ref('g','N',ARRAY[cassie_graph.key_part(1::bigint)])) AS k",
    );

    // Assert
    assert_eq!(lower, vec![vec![Value::Int64(1)]]);
    assert_eq!(upper, lower);
}

#[test]
fn should_bind_graph_constructors_inside_a_table_function_scalar_subquery() {
    // Arrange
    let fixture = started("graph-argument-scalar-subquery", &SOCIAL_SETUP);
    let reference = "cassie_graph.vertex_ref('social','person',\
        ARRAY[cassie_graph.key_part(2::bigint+0::bigint)])";

    // Act
    let direct = fixture.execute(&format!(
        "SELECT count(*) FROM unnest(ARRAY[{reference}]) AS u"
    ));
    let nested = fixture.execute(&format!(
        "SELECT count(*) FROM unnest(ARRAY[(SELECT {reference})]) AS u"
    ));

    // Assert
    let expected = vec![vec![Value::Int64(1)]];
    assert_eq!(
        direct.expect("direct constructor control").rows(),
        &expected
    );
    assert_eq!(
        nested.expect("constructor inside scalar subquery").rows(),
        &expected
    );
}
