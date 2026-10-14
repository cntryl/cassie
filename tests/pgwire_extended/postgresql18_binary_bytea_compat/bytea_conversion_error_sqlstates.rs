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
fn should_report_postgresql_sqlstates_for_bytea_encoding_errors() {
    // Arrange
    let cases = [
        (
            "convert_from_invalid_utf8",
            "SELECT convert_from(decode('c328', 'hex'), 'UTF8')",
            "22021",
        ),
        (
            "convert_to_unrepresentable_latin1",
            "SELECT convert_to('€', 'LATIN1')",
            "22P05",
        ),
    ];
    let mut steps = Vec::with_capacity(cases.len() * 2);
    for (_, sql, _) in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 816 AS recovered",
        )]));
    }

    // Act
    let batches: Vec<Batch> = run_script("issue-816-bytea-conversion-error-sqlstates", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for (index, (case_name, sql, expected_sqlstate)) in cases.iter().enumerate() {
        let error = &batches[index * 2];
        assert_eq!(
            wire::error_code(error).as_deref(),
            Some(*expected_sqlstate),
            "PostgreSQL 18.6 SQLSTATE for {case_name}: {sql}"
        );
        assert!(
            !error
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
            "failed encoding conversion must not return a partial result: {case_name}"
        );
        assert_eq!(
            error.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "failed encoding conversion should restore ReadyForQuery: {case_name}"
        );

        let recovered = &batches[index * 2 + 1];
        assert_eq!(
            wire::error_code(recovered),
            None,
            "following query should recover after {case_name}"
        );
        assert_eq!(
            wire::data_rows(recovered),
            vec![vec![Some("816".to_owned())]],
            "following query result after {case_name}"
        );
    }
}
