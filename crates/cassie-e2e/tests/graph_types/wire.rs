//! Text and binary wire forms of the reserved types, as results and as bind
//! parameters, plus the malformed input classes.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

const KEY_PART: &str = "SELECT oid::text FROM pg_type WHERE typname = 'graph_key_part'";

fn setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    crate::fixture::SOCIAL_SETUP
        .iter()
        .map(|sql| (*sql, Vec::new()))
        .collect()
}

fn key_part_oid(label: &str) -> i32 {
    let cycles = vec![fixture::execute_cycle(KEY_PART, None, 0, false)];
    let batches = fixture::run_cycles(&format!("{label}-oid"), &setup(), cycles);
    wire::data_rows(&batches[0])[0][0]
        .as_deref()
        .and_then(|oid| oid.parse().ok())
        .expect("graph_key_part OID")
}

/// `record_send` of a `graph_key_part` carrying a bigint.
fn bigint_part(value: i64) -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 4];
    bytes.extend_from_slice(&[0, 0, 0, 21, 0, 0, 0, 2, 0, 3]);
    bytes.extend_from_slice(&[0, 0, 0, 20, 0, 0, 0, 8]);
    bytes.extend_from_slice(&value.to_be_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 25, 255, 255, 255, 255]);
    bytes.extend_from_slice(&[0, 0, 11, 134, 255, 255, 255, 255]);
    bytes
}

/// A bound parameter of `oid` in `format`, selected through `sql`.
fn bound(label: &str, sql: &str, oid: i32, format: i16, body: &[u8]) -> fixture::Frames {
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((oid, format, Some(body))),
        0,
        false,
    )];
    fixture::run_cycles(label, &setup(), cycles).remove(0)
}

#[test]
fn should_send_a_key_part_in_text_form() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT cassie_graph.key_part(9007199254740993::bigint)",
        None,
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("graph-wire-text", &setup(), cycles);

    // Assert
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("(3,9007199254740993,,)".to_owned())]]
    );
}

#[test]
fn should_send_a_key_part_in_binary_record_form() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT cassie_graph.key_part(7::bigint)",
        None,
        1,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("graph-wire-binary", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&bigint_part(7))]
    );
}

#[test]
fn should_accept_a_text_key_part_parameter_with_escaped_text() {
    // Arrange
    let oid = key_part_oid("graph-wire-escaped");
    let body = br#"(4,,"a,b ""c"" (d)",)"#;

    // Act
    let batch = bound(
        "graph-wire-escaped",
        "SELECT ($1::cassie_graph.graph_key_part).text_value",
        oid,
        0,
        body,
    );

    // Assert
    assert_eq!(
        wire::data_rows(&batch),
        vec![vec![Some("a,b \"c\" (d)".to_owned())]]
    );
}

#[test]
fn should_accept_a_binary_key_part_parameter_beyond_the_double_precision_range() {
    // Arrange
    let oid = key_part_oid("graph-wire-binary-param");
    let body = bigint_part(9_007_199_254_740_993);

    // Act
    let batch = bound(
        "graph-wire-binary-param",
        "SELECT ($1::cassie_graph.graph_key_part).integer_value::text",
        oid,
        1,
        &body,
    );

    // Assert
    assert_eq!(
        wire::data_rows(&batch),
        vec![vec![Some("9007199254740993".to_owned())]]
    );
}

#[test]
fn should_distinguish_a_null_parameter_from_a_populated_one() {
    // Arrange
    let oid = key_part_oid("graph-wire-null");
    let cycles = vec![fixture::execute_cycle(
        "SELECT $1::cassie_graph.graph_key_part IS NULL",
        Some((oid, 0, None)),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("graph-wire-null", &setup(), cycles);

    // Assert
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("t".to_owned())]]
    );
}

#[test]
fn should_reject_malformed_text_input_with_22p02() {
    // Arrange
    let oid = key_part_oid("graph-wire-22p02");

    // Act
    let batch = bound("graph-wire-22p02", "SELECT $1::text", oid, 0, b"(3,1");

    // Assert
    assert_eq!(wire::error_code(&batch), Some("22P02".to_owned()));
}

#[test]
fn should_reject_malformed_binary_input_with_22p03() {
    // Arrange
    let oid = key_part_oid("graph-wire-22p03");
    let truncated = &bigint_part(1)[..9];

    // Act
    let batch = bound("graph-wire-22p03", "SELECT $1::text", oid, 1, truncated);

    // Assert
    assert_eq!(wire::error_code(&batch), Some("22P03".to_owned()));
}

#[test]
fn should_reject_a_well_formed_invalid_text_parameter_with_22023() {
    // Arrange
    let oid = key_part_oid("graph-wire-22023-text");

    // Act
    let batch = bound(
        "graph-wire-22023-text",
        "SELECT $1::text",
        oid,
        0,
        b"(9,1,,)",
    );

    // Assert
    assert_eq!(wire::error_code(&batch), Some("22023".to_owned()));
}

#[test]
fn should_reject_a_well_formed_invalid_binary_parameter_with_22023() {
    // Arrange
    let oid = key_part_oid("graph-wire-22023-binary");
    let mut body = bigint_part(1);
    body[12..14].copy_from_slice(&[0, 9]);

    // Act
    let batch = bound("graph-wire-22023-binary", "SELECT $1::text", oid, 1, &body);

    // Assert
    assert_eq!(wire::error_code(&batch), Some("22023".to_owned()));
}

#[test]
fn should_round_trip_a_path_through_its_text_form() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        &format!(
            "SELECT {}::text",
            crate::fixture::one_hop(2, 9_007_199_254_740_993, 1)
        ),
        None,
        0,
        false,
    )];
    let first = fixture::run_cycles("graph-wire-path-text-a", &setup(), cycles);
    let text = wire::data_rows(&first[0])[0][0].clone().expect("path text");

    // Act
    let echoed = fixture::run_cycles(
        "graph-wire-path-text-b",
        &setup(),
        vec![fixture::execute_cycle(
            &format!(
                "SELECT '{}'::cassie_graph.graph_path::text",
                text.replace('\'', "''")
            ),
            None,
            0,
            false,
        )],
    );

    // Assert
    assert_eq!(
        wire::data_rows(&echoed[0])[0][0].as_deref(),
        Some(text.as_str())
    );
}
