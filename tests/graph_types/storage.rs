//! Storage of the reserved types in table columns, across a local restart.

use crate::support_composite_types::LocalStore;
use cassie::types::Value;

fn run(cassie: &cassie::app::Cassie, sql: &str) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    match cassie.execute_sql(&session, sql, Vec::new()) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("query failed: {sql}: {error}"),
    }
}

#[test]
fn should_restore_stored_graph_values_after_a_restart() {
    // Arrange
    let store = LocalStore::new("graph_types_restart");
    let first = store.open();
    run(&first, "CREATE TABLE people (id BIGINT PRIMARY KEY)");
    run(&first, "INSERT INTO people VALUES (9007199254740993)");
    run(
        &first,
        "CREATE PROPERTY GRAPH g VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id)) \
         EDGE TABLES (people AS loop KEY (id) SOURCE KEY (id) REFERENCES person (id) \
         DESTINATION KEY (id) REFERENCES person (id) LABEL loop PROPERTIES (id))",
    );
    run(
        &first,
        "CREATE TABLE holders (id INT PRIMARY KEY, r cassie_graph.graph_element_ref)",
    );
    run(
        &first,
        "INSERT INTO holders VALUES (1, cassie_graph.vertex_ref('g', 'person', \
         ARRAY[cassie_graph.key_part(9007199254740993::bigint)]))",
    );
    let before = run(&first, "SELECT r FROM holders");
    let type_before = run(
        &first,
        "SELECT oid FROM pg_type WHERE typname = 'graph_element_ref'",
    );
    drop(first);

    // Act
    let second = store.open();
    let after = run(&second, "SELECT r FROM holders");
    let type_after = run(
        &second,
        "SELECT oid FROM pg_type WHERE typname = 'graph_element_ref'",
    );
    let key = run(
        &second,
        "SELECT ((r).key_parts[1]).integer_value FROM holders",
    );

    // Assert
    assert_eq!(before, after);
    assert_eq!(type_before, type_after);
    assert_eq!(key, vec![vec![Value::Int64(9_007_199_254_740_993)]]);
}

#[test]
fn should_reject_an_invalid_value_written_to_a_stored_column() {
    // Arrange
    let fixture = crate::fixture::started("graph_types_column", &[]);
    fixture.run("CREATE TABLE parts (id INT PRIMARY KEY, p cassie_graph.graph_key_part)");

    // Act
    let state = fixture.sqlstate("INSERT INTO parts VALUES (1, '(4,,,)')");

    // Assert
    assert_eq!(state, "22023");
}

#[test]
fn should_reject_an_invalid_value_written_by_update() {
    // Arrange
    let fixture = crate::fixture::started("graph_types_update", &[]);
    fixture.run("CREATE TABLE parts (id INT PRIMARY KEY, p cassie_graph.graph_key_part)");
    fixture.run("INSERT INTO parts VALUES (1, '(3,1,,)')");

    // Act
    let state = fixture.sqlstate("UPDATE parts SET p = '(3,,,)' WHERE id = 1");

    // Assert
    assert_eq!(state, "22023");
}
