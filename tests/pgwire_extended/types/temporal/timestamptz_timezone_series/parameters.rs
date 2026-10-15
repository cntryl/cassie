//! PostgreSQL 18.6 four-argument `timestamptz` `generate_series` wire contract for #808.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

const TSTZ_TIMEZONE_SERIES_SQL: &str = "SELECT value FROM generate_series(\
    $1::TIMESTAMPTZ, $2::TIMESTAMPTZ, $3::INTERVAL, $4::TEXT) AS series(value)";

#[test]
fn should_bind_four_argument_timestamptz_series_across_dst_with_pgwire_metadata() {
    // Arrange
    let statement = "issue_808_tstz_timezone_series";
    let portal = "issue_808_tstz_timezone_series_portal";
    let parameters = [
        Some(b"2024-03-09 12:00:00-05".as_slice()),
        Some(b"2024-03-12 12:00:00-04".as_slice()),
        Some(b"1 day".as_slice()),
        Some(b"America/New_York".as_slice()),
    ];
    let cycle = vec![
        wire::parse_frame_with_types(
            statement,
            TSTZ_TIMEZONE_SERIES_SQL,
            &[1_184, 1_184, 1_186, 25],
        ),
        wire::describe_statement_frame(statement),
        wire::bind_frame_with_formats(portal, statement, &[0], &parameters, &[0]),
        wire::describe_portal_frame(portal),
        wire::execute_frame(portal),
        wire::sync_frame(),
    ];

    // Act
    let batches = run_script(
        "issue-808-four-argument-timestamptz-series-wire",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
            Step::ThroughReady(cycle),
        ],
    );

    // Assert
    let result = &batches[1];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let parameter_description = result
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("four-argument timestamptz series ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        [1_184, 1_184, 1_186, 25]
    );

    let descriptions = result
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
            (1_184, 8, -1, 0)
        );
    }
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2024-03-09 17:00:00+00".to_owned())],
            vec![Some("2024-03-10 16:00:00+00".to_owned())],
            vec![Some("2024-03-11 16:00:00+00".to_owned())],
            vec![Some("2024-03-12 16:00:00+00".to_owned())],
        ]
    );
}
