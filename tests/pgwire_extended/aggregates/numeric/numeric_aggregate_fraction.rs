use super::{run_script, wire, Step};

#[test]
fn should_encode_exact_numeric_average_as_numeric_binary() {
    // Arrange
    let statement = "numeric_fractional_aggregate";
    let portal = "numeric_fractional_aggregate_binary";
    let sql = "SELECT AVG(value) FROM issue_812_numeric_fractional_aggregate";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_numeric_fractional_aggregate (value NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_numeric_fractional_aggregate VALUES \
             (9007199254740993.1255), (0.0001)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-fractional-aggregate", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    assert_numeric_average_formats(&batches[2], &batches[4], &batches[5]);
}

fn assert_numeric_average_formats(
    text_frames: &[(u8, Vec<u8>)],
    statement_frames: &[(u8, Vec<u8>)],
    binary_frames: &[(u8, Vec<u8>)],
) {
    let text_description = text_frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text aggregate RowDescription");
    let text_columns = wire::parse_row_description(&text_description.1);
    assert_eq!(
        text_columns
            .iter()
            .map(|column| {
                (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                )
            })
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(text_frames),
        vec![vec![Some("4503599627370496.5628".to_owned())]]
    );

    let statement_description = statement_frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared aggregate RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    let binary_description = binary_frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary aggregate portal RowDescription");
    let binary_columns = wire::parse_row_description(&binary_description.1);
    assert_eq!(
        statement_columns
            .iter()
            .map(|column| {
                (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                )
            })
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1, 0)]
    );
    assert_eq!(
        binary_columns
            .iter()
            .map(|column| {
                (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                )
            })
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1, 1)]
    );
    assert_eq!(
        crate::support_type_metadata_contract::data_row_payloads(binary_frames),
        vec![numeric_average_row()],
        "numeric_send payload for the exact AVG(NUMERIC) result"
    );
}

fn numeric_average_row() -> Vec<u8> {
    let average = [
        0x00, 0x05, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x11, 0x97, 0x17, 0x6c, 0x0a, 0xb1, 0x01,
        0xf0, 0x15, 0xfc,
    ];
    let mut row = vec![0x00, 0x01];
    row.extend_from_slice(
        &i32::try_from(average.len())
            .expect("numeric wire value length fits i32")
            .to_be_bytes(),
    );
    row.extend_from_slice(&average);
    row
}
