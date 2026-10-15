use super::{run_script, wire, Step};

#[test]
fn should_format_non_hour_session_zone_offset() {
    // Arrange
    let statement = "offset_format_non_hour";
    let portal = "offset_format_non_hour_portal";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'Asia/Kathmandu'",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT TO_CHAR($1, 'OF') AS offset",
            &[1184],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["2024-01-15 12:34:56+00"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-to-char-minute-zone-offset", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[2];
    assert_eq!(wire::error_code(described), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("timestamptz ParameterDescription")
                .1
        ),
        vec![1184]
    );
    let statement_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared TO_CHAR statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)[0].type_oid,
        25
    );
    let result = &batches[3];
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

#[test]
fn should_render_daylight_hour_offset_with_of_format() {
    // Arrange
    let statement = "daylight_offset_format";
    let portal = "daylight_offset_format_portal";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET TIME ZONE 'America/Los_Angeles'",
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT TO_CHAR($1, 'OF') AS offset",
            &[1184],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["2024-06-01 12:00:00+00"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-815-to-char-daylight-offset", steps);
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[2];
    assert_eq!(wire::error_code(described), None);
    let statement_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared daylight TO_CHAR statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)[0].type_oid,
        25
    );
    let result = &batches[3];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "TO_CHAR offset ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TO_CHAR daylight offset RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (25, -1, -1, 0)
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("-07".to_owned())]]);
}
