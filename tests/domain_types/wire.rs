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
