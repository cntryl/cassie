//! Wire text and binary framing of arrays of named composite types (#1268).

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    vec![
        ("CREATE TYPE address AS (street text, zip int)", Vec::new()),
        ("CREATE TABLE trips (id int, stops address[])", Vec::new()),
        (
            "INSERT INTO trips VALUES \
             (1, ARRAY[ROW('x y', 1)::address, NULL]), (2, '{}'), (3, NULL)",
            Vec::new(),
        ),
    ]
}

fn descriptions(frames: &fixture::Frames) -> Vec<wire::RowDescription> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .flat_map(|(_, body)| wire::parse_row_description(body))
        .collect()
}

#[test]
fn should_send_composite_array_text_with_postgresql_quoting() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT stops FROM trips ORDER BY id",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("composite-array-wire-text", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![
            vec![Some(r#"{"(\"x y\",1)",NULL}"#.to_owned())],
            vec![Some("{}".to_owned())],
            vec![None],
        ]
    );
}

#[test]
fn should_describe_a_composite_array_with_the_array_type_oid() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT stops FROM trips", None, 0, true),
        fixture::execute_cycle(
            "SELECT typarray::text FROM pg_type WHERE typname = 'address'",
            None,
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT oid::text FROM pg_type WHERE typname = '_address'",
            None,
            0,
            true,
        ),
    ];

    // Act
    let batches = fixture::run_cycles("composite-array-wire-oid", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    let described = descriptions(&batches[0])[0].type_oid;
    let array_row = wire::data_rows(&batches[2])[0][0].clone();
    assert!(described > 16_383, "the array type uses an allocated OID");
    assert_eq!(Some(described.to_string()), array_row);
    assert_eq!(descriptions(&batches[0])[0].type_size, -1);
}

#[test]
fn should_send_binary_composite_arrays_in_array_send_layout() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT stops FROM trips WHERE id = 1", None, 1, false),
        fixture::execute_cycle(
            "SELECT oid::text FROM pg_type WHERE typname = 'address'",
            None,
            0,
            true,
        ),
    ];
    let record: Vec<u8> = [
        &[0, 0, 0, 2][..],
        &[0, 0, 0, 25, 0, 0, 0, 3, b'x', b' ', b'y'],
        &[0, 0, 0, 23, 0, 0, 0, 4, 0, 0, 0, 1],
    ]
    .concat();

    // Act
    let batches = fixture::run_cycles("composite-array-wire-binary", &setup(), cycles);
    let element_oid: u32 = wire::data_rows(&batches[1])[0][0]
        .as_deref()
        .and_then(|oid| oid.parse().ok())
        .expect("address type OID");
    let expected: Vec<u8> = [
        &[0, 0, 0, 1, 0, 0, 0, 1][..],
        &element_oid.to_be_bytes(),
        &[0, 0, 0, 2, 0, 0, 0, 1],
        &u32::try_from(record.len()).expect("length").to_be_bytes(),
        &record,
        &[255, 255, 255, 255],
    ]
    .concat();

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&expected)]
    );
}

#[test]
fn should_send_an_empty_binary_composite_array_with_zero_dimensions() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT stops FROM trips WHERE id = 2",
        None,
        1,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("composite-array-wire-empty", &setup(), cycles);
    let payloads = fixture::data_row_payloads(&batches[0]);
    let field = &payloads[0][6..];

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(field.len(), 12);
    assert_eq!(field[..8], [0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn should_accept_a_text_parameter_cast_to_a_composite_array() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT (($1::address[])[2]).zip",
        Some((0, 0, Some(br#"{"(\"a b\",7)","(c,9)"}"#.as_slice()))),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("composite-array-wire-parameter", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("9".to_owned())]]
    );
}
