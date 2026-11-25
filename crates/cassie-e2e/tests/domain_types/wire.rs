//! `RowDescription` reports the base type OID of a domain column.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    vec![
        ("CREATE DOMAIN posint AS int CHECK (VALUE > 0)", Vec::new()),
        ("CREATE TABLE items (id int, qty posint)", Vec::new()),
        ("INSERT INTO items VALUES (1, 4)", Vec::new()),
    ]
}

#[test]
fn should_describe_a_domain_column_with_the_base_oid() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT qty FROM items",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("domain-wire-oid", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    let description = batches[0]
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .next()
        .expect("a RowDescription");
    assert_eq!(
        description[0].type_oid, 23,
        "int4 base OID, not the domain OID"
    );
    assert_eq!(description[0].type_size, 4);
}

#[test]
fn should_send_domain_values_as_base_text() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT qty FROM items",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("domain-wire-text", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("4".to_owned())]]
    );
}

fn identity_setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    [
        "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
        "CREATE TYPE mood AS ENUM ('calm', 'busy')",
        "CREATE TYPE pair AS (a int, b text)",
        "CREATE TABLE items (id int, qty posint)",
        "CREATE SCHEMA hid",
        "CREATE DOMAIN hid.secret AS int",
        "CREATE DOMAIN price AS numeric DEFAULT 1.50",
        "CREATE TABLE priced (id int, amount price)",
        "INSERT INTO priced (id) VALUES (1)",
    ]
    .into_iter()
    .map(|sql| (sql, Vec::new()))
    .collect()
}

/// The single text value of a one-row, one-column simple-protocol query.
fn identity_value(label: &str, sql: &str) -> Option<String> {
    let batches = fixture::run_cycles(
        label,
        &identity_setup(),
        vec![vec![wire::simple_query_frame(sql)]],
    );
    fixture::assert_success(&batches[0]);
    wire::data_rows(&batches[0])
        .into_iter()
        .next()
        .and_then(|row| row.into_iter().next())
        .flatten()
}

#[test]
fn should_format_a_domain_oid_as_the_domain_name() {
    // Arrange
    let sql = "SELECT format_type('posint'::regtype::oid, -1)";

    // Act
    let value = identity_value("domain-format-type-domain", sql);

    // Assert
    assert_eq!(value.as_deref(), Some("posint"));
}

#[test]
fn should_format_an_enum_oid_as_the_enum_name() {
    // Arrange
    let sql = "SELECT format_type('mood'::regtype::oid, -1)";

    // Act
    let value = identity_value("domain-format-type-enum", sql);

    // Assert
    assert_eq!(value.as_deref(), Some("mood"));
}

#[test]
fn should_format_a_composite_oid_as_the_type_name() {
    // Arrange
    let sql = "SELECT format_type('pair'::regtype::oid, -1)";

    // Act
    let value = identity_value("domain-format-type-composite", sql);

    // Assert
    assert_eq!(value.as_deref(), Some("pair"));
}

#[test]
fn should_format_a_row_type_oid_as_the_table_name() {
    // Arrange
    let sql = "SELECT format_type('items'::regtype::oid, -1)";

    // Act
    let value = identity_value("domain-format-type-row", sql);

    // Assert
    assert_eq!(value.as_deref(), Some("items"));
}

#[test]
fn should_schema_qualify_a_formatted_type_outside_the_search_path() {
    // Arrange
    let sql = "SELECT format_type('hid.secret'::regtype::oid, -1)";

    // Act
    let value = identity_value("domain-format-type-qualified", sql);

    // Assert
    assert_eq!(value.as_deref(), Some("hid.secret"));
}

#[test]
fn should_keep_the_written_scale_of_a_numeric_domain_default() {
    // Arrange
    let sql = "SELECT amount FROM priced";

    // Act
    let value = identity_value("domain-default-scale", sql);

    // Assert
    assert_eq!(value.as_deref(), Some("1.50"));
}
