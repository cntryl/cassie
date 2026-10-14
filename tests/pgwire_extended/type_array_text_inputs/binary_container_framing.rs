//! PostgreSQL binary array outer-container framing over extended Bind.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

fn int4_array_binary_cycle(body: &[u8]) -> fixture::Cycle {
    fixture::execute_cycle(
        "SELECT $1::INT4[] AS value",
        Some((1007, 1, Some(body))),
        0,
        false,
    )
}

fn assert_pgwire_error_and_recovery(
    rejected: &fixture::Frames,
    recovery: &fixture::Frames,
    state: &str,
    message: &str,
) {
    assert_eq!(wire::error_code(rejected).as_deref(), Some(state));
    assert_eq!(rejected.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    let fields = rejected
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("malformed binary array ErrorResponse");
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some(message)
    );
    assert!(fixture::data_row_payloads(rejected).is_empty());
    assert!(!rejected.iter().any(|(tag, _)| matches!(*tag, b'T' | b'C')));
    assert_eq!(
        rejected.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );

    fixture::assert_success(recovery);
    assert_eq!(
        fixture::data_row_payloads(recovery),
        vec![fixture::one_field_row(b"45")]
    );
}

#[test]
fn should_reject_truncated_binary_array_dimension_descriptor_then_recover() {
    // Arrange
    let valid = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 7,
    ];
    let truncated_dimension = [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 23];
    let cycles = vec![
        int4_array_binary_cycle(&valid),
        int4_array_binary_cycle(&truncated_dimension),
        fixture::execute_cycle("SELECT 45 AS recovery", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-array-truncated-dimension-recovery", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 3);
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{7}".to_owned())]]
    );
    assert_pgwire_error_and_recovery(
        &batches[1],
        &batches[2],
        "08P01",
        "insufficient data left in message",
    );
}

#[test]
fn should_reject_truncated_binary_array_element_contents_then_recover() {
    // Arrange
    let valid = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 7,
    ];
    let truncated_element = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 4, 0, 7,
    ];
    let cycles = vec![
        int4_array_binary_cycle(&valid),
        int4_array_binary_cycle(&truncated_element),
        fixture::execute_cycle("SELECT 45 AS recovery", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-array-truncated-element-recovery", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 3);
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{7}".to_owned())]]
    );
    assert_pgwire_error_and_recovery(
        &batches[1],
        &batches[2],
        "22P03",
        "insufficient data left in message",
    );
}

#[test]
fn should_reject_trailing_byte_after_empty_binary_array_then_recover() {
    // Arrange
    let valid_empty = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23];
    let empty_with_trailing_byte = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23, 0];
    let cycles = vec![
        int4_array_binary_cycle(&valid_empty),
        int4_array_binary_cycle(&empty_with_trailing_byte),
        fixture::execute_cycle("SELECT 45 AS recovery", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-array-empty-trailing-byte-recovery", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 3);
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{}".to_owned())]]
    );
    assert_pgwire_error_and_recovery(
        &batches[1],
        &batches[2],
        "22P03",
        "incorrect binary data format in bind parameter 1",
    );
}
