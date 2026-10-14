use super::{run_script, wire, Step};

#[test]
fn should_format_non_hour_session_zone_offset() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'Asia/Kathmandu'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TO_CHAR(TIMESTAMPTZ '2024-01-15 12:34:56+00', 'OF') AS offset",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-to-char-minute-zone-offset", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TO_CHAR offset RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code,
        ),
        (25, -1, -1, 0)
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("+05:45".to_owned())]]
    );
}
