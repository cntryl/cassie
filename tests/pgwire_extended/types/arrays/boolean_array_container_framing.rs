//! PostgreSQL boolean-array binary container framing for #807.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_classify_trailing_boolean_array_bytes_as_malformed_binary_data() {
    // Arrange
    let valid = [
        0, 0, 0, 1, // dimensions
        0, 0, 0, 0, // no NULL elements
        0, 0, 0, 16, // BOOL element OID
        0, 0, 0, 1, // element count
        0, 0, 0, 1, // lower bound
        0, 0, 0, 1, // element byte length
        1, // true
    ];
    let malformed = [
        0, 0, 0, 1, // dimensions
        0, 0, 0, 0, // no NULL elements
        0, 0, 0, 16, // BOOL element OID
        0, 0, 0, 1, // element count
        0, 0, 0, 1, // lower bound
        0, 0, 0, 1, // element byte length
        1, 0, // true followed by an unconsumed byte
    ];
    let valid = fixture::execute_cycle(
        "SELECT $1::BOOL[] AS value",
        Some((1_000, 1, Some(&valid))),
        0,
        false,
    );
    let malformed = fixture::execute_cycle(
        "SELECT $1::BOOL[] AS value",
        Some((1_000, 1, Some(&malformed))),
        0,
        false,
    );
    let recovery = fixture::execute_cycle("SELECT 45 AS recovery", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-807-boolean-array-trailing-byte-recovery",
        &[],
        vec![valid, malformed, recovery],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{t}".to_owned())]]
    );

    let rejected = &batches[1];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("22P03"));
    assert_eq!(rejected.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert_eq!(
        rejected
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .and_then(|fields| {
                fields
                    .into_iter()
                    .find(|(kind, _)| *kind == 'M')
                    .map(|(_, value)| value)
            })
            .as_deref(),
        Some("incorrect binary data format in bind parameter 1")
    );
    assert_eq!(fixture::data_row_payloads(rejected), Vec::<Vec<u8>>::new());
    assert!(!rejected.iter().any(|(tag, _)| matches!(*tag, b'T' | b'C')));
    assert_eq!(
        rejected.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("45".to_owned())]]
    );
}
