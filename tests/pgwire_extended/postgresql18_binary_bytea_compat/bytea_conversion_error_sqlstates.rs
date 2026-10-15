//! PostgreSQL 18 bytea encoding conversion errors tracked by #816.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

type Batch = Vec<(u8, Vec<u8>)>;

#[test]
fn should_round_trip_utf8_text_through_bytea_conversions_over_pgwire() {
    // Arrange
    let sql = "SELECT convert_to('café', 'UTF8') AS encoded, \
                     convert_from(decode('636166c3a9', 'hex'), 'UTF8') AS decoded";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-816-bytea-convert-to-from", steps);
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .expect("conversion RowDescription");
    assert_eq!(
        description
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(17, -1, 0), (25, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some(r"\x636166c3a9".to_owned()),
            Some("café".to_owned())
        ]]
    );
}

#[test]
fn should_report_invalid_utf8_bytea_conversion_sqlstate() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            r"SELECT convert_from(CAST('\xc328' AS BYTEA), 'UTF8')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 816 AS recovered")]),
    ];

    // Act
    let batches: Vec<Batch> = run_script("issue-816-bytea-invalid-utf8-error", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    let error = &batches[0];
    assert_eq!(
        wire::error_code(error).as_deref(),
        Some("22021"),
        "PostgreSQL 18.6 invalid UTF8 conversion ErrorResponse fields: {:?}",
        error
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert!(
        !error
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
        "failed encoding conversion must not return partial rows"
    );
    assert_eq!(
        error.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..]),
        "failed encoding conversion should restore ReadyForQuery"
    );

    let recovered = &batches[1];
    assert_eq!(wire::error_code(recovered), None);
    assert_eq!(
        wire::data_rows(recovered),
        vec![vec![Some("816".to_owned())]]
    );
}
