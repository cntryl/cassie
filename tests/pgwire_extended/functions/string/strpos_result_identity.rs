//! PostgreSQL result type and wire encoding for `strpos` under #803.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_unicode_strpos_int4_identity_across_formats() {
    // Arrange
    let statement = "strpos_unicode";
    let sql = "SELECT strpos('Aé🙂Z', '🙂') AS character_position";
    let batches = run_script(
        "issue-803-strpos-int4-wire-identity",
        vec![
            Step::ParseFlush(wire::parse_frame(statement, sql)),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("strpos_text", statement, &[], &[], &[0]),
                wire::describe_portal_frame("strpos_text"),
                wire::execute_frame("strpos_text"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("strpos_binary", statement, &[], &[], &[1]),
                wire::describe_portal_frame("strpos_binary"),
                wire::execute_frame("strpos_binary"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let text_result = &batches[2];
    let binary_result = &batches[3];

    // Assert
    for (stage, frames) in [
        ("Parse", parsed),
        ("Describe", described),
        ("text Execute", text_result),
        ("binary Execute", binary_result),
    ] {
        let errors = frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>();
        assert_eq!(
            wire::error_code(frames),
            None,
            "strpos {stage} ErrorResponse: {errors:?}"
        );
    }
    let statement_columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("strpos statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_columns.1)[0];
    assert_eq!(statement_column.name, "character_position");
    assert_eq!(statement_column.type_oid, 23);

    let text_description = text_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("text strpos portal RowDescription");
    let text_column = &wire::parse_row_description(&text_description.1)[0];
    assert_eq!((text_column.type_oid, text_column.format_code), (23, 0));
    assert_eq!(
        wire::data_rows(text_result),
        vec![vec![Some("3".to_owned())]]
    );

    let binary_description = binary_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary strpos portal RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!((binary_column.type_oid, binary_column.format_code), (23, 1));
    let data_row = binary_result
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary strpos DataRow");
    assert_eq!(
        i16::from_be_bytes(data_row.1[0..2].try_into().expect("field count")),
        1
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[2..6].try_into().expect("field length")),
        4
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[6..10].try_into().expect("INT4 value")),
        3
    );
}
