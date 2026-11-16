//! Constructors, accessors and the JSON export.

use crate::fixture::started;
use cassie::types::Value;

#[test]
fn should_build_a_key_part_from_each_exact_type() {
    // Arrange
    let fixture = started("graph_key_part_types", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT (cassie_graph.key_part(7::smallint)).type_tag, \
                (cassie_graph.key_part(7::integer)).type_tag, \
                (cassie_graph.key_part(7::bigint)).type_tag, \
                (cassie_graph.key_part('a'::text)).type_tag, \
                (cassie_graph.key_part('00000000-0000-0000-0000-000000000001'::uuid)).type_tag",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(1),
            Value::Int64(2),
            Value::Int64(3),
            Value::Int64(4),
            Value::Int64(5),
        ]]
    );
}
