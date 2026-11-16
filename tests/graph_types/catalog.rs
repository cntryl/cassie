//! Discovery of the reserved `cassie_graph` schema through `pg_type`, `pg_class`
//! and `pg_attribute`.

use crate::fixture::started;
use cassie::types::Value;

#[test]
fn should_publish_the_three_graph_types_in_pg_type() {
    // Arrange
    let fixture = started("graph_types_pg_type", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT t.typname, t.typtype FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
         WHERE n.nspname = 'cassie_graph' AND t.typtype = 'c' ORDER BY t.typname",
    );

    // Assert
    let names = rows
        .iter()
        .map(|row| row[0].clone())
        .collect::<Vec<Value>>();
    assert_eq!(
        names,
        vec![
            Value::String("graph_element_ref".into()),
            Value::String("graph_key_part".into()),
            Value::String("graph_path".into()),
        ]
    );
}
