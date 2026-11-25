//! PostgreSQL 18.6 TIME 24:00:00 text and binary wire value for #814.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_time_twenty_four_endpoint_across_wire_formats() {
    // Arrange
    let statement = "time_endpoint_parameter";
    let sql = "SELECT $1 AS endpoint";
    let binary_endpoint = 86_400_000_000_i64.to_be_bytes();
    let text_endpoint = b"24:00:00";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TIME '24:00:00' AS endpoint",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[1083])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "binary_input_text_output",
                statement,
                &[1],
                &[Some(&binary_endpoint[..])],
                &[0],
            ),
            wire::describe_portal_frame("binary_input_text_output"),
            wire::execute_frame("binary_input_text_output"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "text_input_binary_output",
                statement,
                &[0],
                &[Some(&text_endpoint[..])],
                &[1],
            ),
            wire::describe_portal_frame("text_input_binary_output"),
            wire::execute_frame("text_input_binary_output"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-814-time-twenty-four-wire-endpoint", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    assert_time_endpoint_descriptor(&batches[0], 0);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("24:00:00".to_owned())]]
    );
    assert_eq!(
        wire::parse_parameter_description(
            &batches[2]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("temporal ParameterDescription")
                .1
        ),
        vec![1083]
    );

    let binary_input_text_output = &batches[3];
    assert_time_endpoint_descriptor(binary_input_text_output, 0);
    assert_eq!(
        wire::data_rows(binary_input_text_output),
        vec![vec![Some("24:00:00".to_owned())]]
    );

    let text_input_binary_output = &batches[4];
    assert_time_endpoint_descriptor(text_input_binary_output, 1);
    assert_eq!(
        super::binary_row(text_input_binary_output),
        vec![binary_endpoint.to_vec()]
    );
}

fn assert_time_endpoint_descriptor(frames: &[(u8, Vec<u8>)], format_code: i16) {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TIME endpoint RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code,
        ),
        (1083, 8, -1, format_code)
    );
}
