//! `Bind` parameters typed by a composite or composite array OID, in text and
//! binary (#1268).

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    vec![
        ("CREATE TYPE address AS (street text, zip int)", Vec::new()),
        ("CREATE TABLE trips (id int, stops address[])", Vec::new()),
    ]
}

/// The allocated OIDs of `address` and `address[]`. Allocation is
/// deterministic for a fresh store with this setup.
fn type_oids(label: &str) -> (i32, i32) {
    let cycles = vec![fixture::execute_cycle(
        "SELECT oid::text || ',' || typarray::text FROM pg_type WHERE typname = 'address'",
        None,
        0,
        true,
    )];
    let batches = fixture::run_cycles(&format!("{label}-oids"), &setup(), cycles);
    let row = wire::data_rows(&batches[0])[0][0]
        .clone()
        .expect("type OIDs");
    let (element, array) = row.split_once(',').expect("two OIDs");
    (
        element.parse().expect("element OID"),
        array.parse().expect("array OID"),
    )
}

/// `record_send` of an `address`.
fn record(street: &str, zip: i32) -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 2, 0, 0, 0, 25];
    bytes.extend_from_slice(&u32::try_from(street.len()).expect("length").to_be_bytes());
    bytes.extend_from_slice(street.as_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 23, 0, 0, 0, 4]);
    bytes.extend_from_slice(&zip.to_be_bytes());
    bytes
}

/// `array_send` of one dimension over `elements` (`None` is SQL NULL).
fn array(element_oid: i32, elements: &[Option<Vec<u8>>]) -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 1, 0, 0, 0, 1];
    bytes.extend_from_slice(&element_oid.to_be_bytes());
    bytes.extend_from_slice(&u32::try_from(elements.len()).expect("length").to_be_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 1]);
    for element in elements {
        match element {
            Some(record) => {
                bytes
                    .extend_from_slice(&u32::try_from(record.len()).expect("length").to_be_bytes());
                bytes.extend_from_slice(record);
            }
            None => bytes.extend_from_slice(&[255, 255, 255, 255]),
        }
    }
    bytes
}

fn single_text(label: &str, sql: &str, parameter: (i32, i16, &[u8])) -> Vec<Vec<Option<String>>> {
    let (oid, format, body) = parameter;
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((oid, format, Some(body))),
        0,
        false,
    )];
    let batches = fixture::run_cycles(label, &setup(), cycles);
    fixture::assert_success(&batches[0]);
    wire::data_rows(&batches[0])
}

#[test]
fn should_accept_a_binary_record_parameter() {
    // Arrange
    let (element_oid, _) = type_oids("composite-param-record");
    let body = record("a b", 7);

    // Act
    let rows = single_text(
        "composite-param-record",
        "SELECT ($1::address).street || '/' || ($1::address).zip::text",
        (element_oid, 1, &body),
    );

    // Assert
    assert_eq!(rows, vec![vec![Some("a b/7".to_owned())]]);
}

#[test]
fn should_accept_a_binary_composite_array_parameter() {
    // Arrange
    let (element_oid, array_oid) = type_oids("composite-param-array");
    let body = array(
        element_oid,
        &[Some(record("a \"b\"", 7)), None, Some(record("c,d", 9))],
    );

    // Act
    let rows = single_text(
        "composite-param-array",
        "SELECT $1::address[]::text",
        (array_oid, 1, &body),
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![Some(
            r#"{"(\"a \"\"b\"\"\",7)",NULL,"(\"c,d\",9)"}"#.to_owned()
        )]]
    );
}

#[test]
fn should_store_a_binary_composite_array_parameter_in_a_column() {
    // Arrange
    let (element_oid, array_oid) = type_oids("composite-param-store");
    let body = array(element_oid, &[Some(record("x", 1)), Some(record("y", 2))]);
    let cycles = vec![
        fixture::execute_cycle(
            "INSERT INTO trips VALUES (1, $1)",
            Some((array_oid, 1, Some(&body))),
            0,
            false,
        ),
        fixture::execute_cycle(
            "SELECT (stops[2]).street FROM trips WHERE id = 1",
            None,
            0,
            true,
        ),
    ];

    // Act
    let batches = fixture::run_cycles("composite-param-store", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("y".to_owned())]]
    );
}

#[test]
fn should_accept_a_text_parameter_declared_as_a_composite_array() {
    // Arrange
    let (_, array_oid) = type_oids("composite-param-text");

    // Act
    let rows = single_text(
        "composite-param-text",
        "SELECT (($1::address[])[1]).zip::text",
        (array_oid, 0, br#"{"(a,5)"}"#),
    );

    // Assert
    assert_eq!(rows, vec![vec![Some("5".to_owned())]]);
}

#[test]
fn should_reject_malformed_binary_composite_input_with_sqlstate_22p03() {
    // Arrange
    let (element_oid, array_oid) = type_oids("composite-param-malformed");
    let truncated = array(element_oid, &[Some(record("a", 1))]);
    let truncated = &truncated[..truncated.len() - 2];
    let wrong_element = array(23, &[Some(record("a", 1))]);
    let short_record = [0_u8, 0, 0, 2, 0, 0, 0, 25];
    let cycle = |oid: i32, body: &[u8]| {
        vec![fixture::execute_cycle(
            "SELECT $1::text",
            Some((oid, 1, Some(body))),
            0,
            false,
        )]
    };

    // Act
    let codes = [
        (array_oid, truncated),
        (array_oid, wrong_element.as_slice()),
        (element_oid, short_record.as_slice()),
    ]
    .map(|(oid, body)| {
        let batches = fixture::run_cycles("composite-param-malformed", &setup(), cycle(oid, body));
        wire::error_code(&batches[0])
    });

    // Assert
    assert_eq!(codes, [(); 3].map(|()| Some("22P03".to_owned())));
}

#[test]
fn should_keep_awkward_text_fields_of_binary_parameters_intact() {
    // Arrange
    let (element_oid, array_oid) = type_oids("composite-param-fidelity");
    let streets = ["a\"b\\c,d (e) {f}", "", "NULL", " padded ", "null"];
    let body = array(
        element_oid,
        &streets
            .iter()
            .map(|street| Some(record(street, 1)))
            .collect::<Vec<_>>(),
    );
    let cycles = (1..=streets.len())
        .map(|position| {
            fixture::execute_cycle(
                &format!("SELECT coalesce((($1::address[])[{position}]).street, '<sql null>')"),
                Some((array_oid, 1, Some(&body))),
                0,
                false,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("composite-param-fidelity", &setup(), cycles);
    let returned = batches
        .iter()
        .map(|batch| wire::data_rows(batch)[0][0].clone().expect("street"))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(returned, streets);
}

#[test]
fn should_reject_an_array_header_that_claims_more_elements_than_it_carries() {
    // Arrange
    let (element_oid, array_oid) = type_oids("composite-param-header");
    let mut oversized = vec![0, 0, 0, 2, 0, 0, 0, 0];
    oversized.extend_from_slice(&element_oid.to_be_bytes());
    oversized.extend_from_slice(&[0x7f, 0xff, 0xff, 0xff, 0, 0, 0, 1]);
    oversized.extend_from_slice(&[0x7f, 0xff, 0xff, 0xff, 0, 0, 0, 1]);
    let mut negative = vec![0, 0, 0, 1, 0, 0, 0, 0];
    negative.extend_from_slice(&element_oid.to_be_bytes());
    negative.extend_from_slice(&[0xff, 0xff, 0xff, 0xfe, 0, 0, 0, 1]);
    let too_many_dimensions = [0_u8, 0, 0, 99, 0, 0, 0, 0, 0, 0, 0, 0];
    let cycle = |body: &[u8]| {
        vec![fixture::execute_cycle(
            "SELECT $1::text",
            Some((array_oid, 1, Some(body))),
            0,
            false,
        )]
    };

    // Act
    let codes = [
        oversized.as_slice(),
        negative.as_slice(),
        too_many_dimensions.as_slice(),
    ]
    .map(|body| {
        let batches = fixture::run_cycles("composite-param-header", &setup(), cycle(body));
        wire::error_code(&batches[0])
    });

    // Assert
    assert_eq!(codes, [(); 3].map(|()| Some("22P03".to_owned())));
}
