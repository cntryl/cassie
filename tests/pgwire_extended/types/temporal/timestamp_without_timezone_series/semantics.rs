//! Timestamp-without-time-zone series retains its PostgreSQL result identity (#808).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_timestamp_series_result_over_pgwire() {
    // Arrange
    let statement = "issue_808_timestamp_series";
    let portal = "issue_808_timestamp_series_portal";
    let sql = "SELECT value FROM generate_series($1, $2, $3) AS series(value) ORDER BY value";
    let parameters = ["2024-01-01 23:30:00", "2024-01-02 01:30:00", "1 hour"];
    let cycle = vec![
        wire::parse_frame_with_types(statement, sql, &[1_114, 1_114, 1_186]),
        wire::describe_statement_frame(statement),
        wire::bind_frame(portal, statement, &parameters),
        wire::describe_portal_frame(portal),
        wire::execute_frame(portal),
        wire::sync_frame(),
    ];

    // Act
    let batches = run_script(
        "issue-808-timestamp-without-time-zone-series",
        vec![Step::ThroughReady(cycle)],
    );
    let frames = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(frames),
        None,
        "timestamp-series ErrorResponse fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let parameter_description = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("timestamp-series ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        [1_114, 1_114, 1_186]
    );

    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for columns in descriptions {
        assert_eq!(columns.len(), 1);
        assert_eq!(
            (
                columns[0].type_oid,
                columns[0].type_size,
                columns[0].type_mod,
                columns[0].format_code,
            ),
            (1_114, 8, -1, 0)
        );
    }
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("2024-01-01 23:30:00".to_owned())],
            vec![Some("2024-01-02 00:30:00".to_owned())],
            vec![Some("2024-01-02 01:30:00".to_owned())],
        ]
    );
}
