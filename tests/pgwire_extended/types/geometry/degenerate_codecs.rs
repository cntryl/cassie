//! Degenerate geometry values in text and binary pgwire formats.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_degenerate_geometry_values_given_text_and_binary_formats_when_values_roundtrip()
{
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let sql = "SELECT '<(0,0),0>'::circle AS zero_radius";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame("zero_circle", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("zero_circle_portal", "zero_circle", &[], &[], &[1]),
            wire::describe_portal_frame("zero_circle_portal"),
            wire::execute_frame("zero_circle_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-zero-radius-circle-codecs", steps);

    // Assert
    assert_eq!(batches.len(), 3);
    let text = &batches[0];
    assert_eq!(wire::error_code(text), None);
    let text_description = text
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text circle RowDescription");
    let text_column = &wire::parse_row_description(&text_description.1)[0];
    assert_eq!(
        (
            text_column.type_oid,
            text_column.type_size,
            text_column.type_mod,
            text_column.format_code
        ),
        (718, 24, -1, 0)
    );
    assert_eq!(
        wire::data_rows(text),
        vec![vec![Some("<(0,0),0>".to_owned())]]
    );

    let parse = &batches[1];
    assert_eq!(wire::error_code(parse), None);
    assert_eq!(parse.first().map(|(tag, _)| *tag), Some(b'1'));

    let binary = &batches[2];
    assert_eq!(wire::error_code(binary), None);
    let binary_description = binary
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary circle RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(
        (
            binary_column.type_oid,
            binary_column.type_size,
            binary_column.type_mod,
            binary_column.format_code
        ),
        (718, 24, -1, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(binary),
        vec![fixture::one_field_row(&[0; 24])]
    );
}
