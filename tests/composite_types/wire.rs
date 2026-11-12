//! Record text and binary framing, RowDescription OIDs and parameter input.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

const SETUP: [(&str, Vec<cassie::types::Value>); 2] = [
    ("CREATE TYPE address AS (street text, zip int)", Vec::new()),
    ("CREATE TABLE homes (id int, a address)", Vec::new()),
];

fn descriptions(frames: &fixture::Frames) -> Vec<Vec<wire::RowDescription>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect()
}

fn seeded_setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    let mut setup = SETUP.to_vec();
    setup.push((
        "INSERT INTO homes VALUES (1, ROW('x y', 1)), (2, NULL), (3, ROW(NULL, NULL))",
        Vec::new(),
    ));
    setup
}

#[test]
fn should_send_record_text_with_postgresql_quoting() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT a FROM homes ORDER BY id",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("composite-wire-text", &seeded_setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![
            vec![Some("(\"x y\",1)".to_owned())],
            vec![None],
            vec![Some("(,)".to_owned())],
        ]
    );
}

#[test]
fn should_describe_named_records_with_the_allocated_oid_and_anonymous_with_2249() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT a FROM homes", None, 0, true),
        fixture::execute_cycle("SELECT ROW(1, 'x')", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("composite-wire-oids", &seeded_setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    let named = &descriptions(&batches[0])[0][0];
    let anonymous = &descriptions(&batches[1])[0][0];
    assert!(
        named.type_oid > 16_383,
        "named composite uses an allocated OID"
    );
    assert_eq!(named.type_size, -1);
    assert_eq!(anonymous.type_oid, 2249);
}

#[test]
fn should_send_binary_records_with_field_count_oids_and_lengths() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT a FROM homes WHERE id = 1",
        None,
        1,
        false,
    )];
    let expected: Vec<u8> = [
        &[0, 0, 0, 2][..],
        &[0, 0, 0, 25, 0, 0, 0, 3, b'x', b' ', b'y'],
        &[0, 0, 0, 23, 0, 0, 0, 4, 0, 0, 0, 1],
    ]
    .concat();

    // Act
    let batches = fixture::run_cycles("composite-wire-binary", &seeded_setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&expected)]
    );
}

#[test]
fn should_send_a_binary_null_field_as_length_minus_one() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT a FROM homes WHERE id = 3",
        None,
        1,
        false,
    )];
    let expected: Vec<u8> = [
        &[0, 0, 0, 2][..],
        &[0, 0, 0, 25, 255, 255, 255, 255],
        &[0, 0, 0, 23, 255, 255, 255, 255],
    ]
    .concat();

    // Act
    let batches = fixture::run_cycles("composite-wire-binary-null", &seeded_setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&expected)]
    );
}

#[test]
fn should_accept_a_text_parameter_cast_to_a_named_composite() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT ($1::address).zip",
        Some((0, 0, Some(b"(\"a b\",7)".as_slice()))),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("composite-wire-parameter", &seeded_setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("7".to_owned())]]
    );
}
