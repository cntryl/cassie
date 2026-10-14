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
    let text_description = batches[2]
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
        wire::data_rows(&batches[2]),
        vec![vec![Some("4503599627370496.5628".to_owned())]]
    );

    let statement_description = batches[4]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared aggregate RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    let binary_description = batches[5]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary aggregate portal RowDescription");
    let binary_columns = wire::parse_row_description(&binary_description.1);
    let average = [
        0x00, 0x05, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x11, 0x97, 0x17, 0x6c, 0x0a, 0xb1, 0x01,
        0xf0, 0x15, 0xfc,
    ];
    let mut expected_row = vec![0x00, 0x01];
    expected_row.extend_from_slice(
        &i32::try_from(average.len())
            .expect("numeric wire value length fits i32")
            .to_be_bytes(),
    );
    expected_row.extend_from_slice(&average);
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
        crate::support_type_metadata_contract::data_row_payloads(&batches[5]),
        vec![expected_row],
        "numeric_send payload for the exact AVG(NUMERIC) result"
    );
}
