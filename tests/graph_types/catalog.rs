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

#[test]
fn should_publish_the_array_types_in_pg_type() {
    // Arrange
    let fixture = started("graph_types_arrays", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT typname FROM pg_type WHERE typname IN \
         ('_graph_key_part', '_graph_element_ref', '_graph_path') ORDER BY typname",
    );

    // Assert
    assert_eq!(rows.len(), 3);
}

#[test]
fn should_publish_composite_relations_in_pg_class() {
    // Arrange
    let fixture = started("graph_types_pg_class", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT relname, relkind FROM pg_class WHERE relkind = 'c' AND relname IN \
         ('graph_key_part', 'graph_element_ref', 'graph_path') ORDER BY relname",
    );

    // Assert
    let kinds = rows.iter().map(|row| row[1].clone()).collect::<Vec<_>>();
    assert_eq!(kinds, vec![Value::String("c".into()); 3]);
}

#[test]
fn should_publish_the_attributes_of_graph_path_in_pg_attribute() {
    // Arrange
    let fixture = started("graph_types_pg_attribute", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT attname FROM pg_attribute a JOIN pg_type t ON a.attrelid_oid = t.typrelid \
         WHERE t.typname = 'graph_path' AND a.attnum > 0 ORDER BY a.attnum",
    );

    // Assert
    let names = rows.iter().map(|row| row[0].clone()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            Value::String("vertices".into()),
            Value::String("edges".into()),
            Value::String("directions".into()),
        ]
    );
}

#[test]
fn should_keep_the_type_identities_across_a_second_bootstrap() {
    // Arrange
    let fixture = started("graph_types_idempotent", &[]);
    let before = fixture.rows("SELECT oid FROM pg_type WHERE typname = 'graph_path'");

    // Act
    let outcome = fixture.cassie.ensure_graph_schema("postgres");
    let after = fixture.rows("SELECT oid FROM pg_type WHERE typname = 'graph_path'");

    // Assert
    assert!(outcome.is_ok());
    assert_eq!(before, after);
}
