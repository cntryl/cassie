//! Ordinary temporal abbreviation input through prepared text parameters.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_decode_default_dynamic_abbreviations_from_text_parameters() {
    // Arrange
    let statement = "default_dynamic_abbreviation";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1::DATE AS date_value, $2::TIMESTAMP AS timestamp_value, $3::TIMESTAMPTZ AS timestamptz_value",
            &[25, 25, 25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "clt_values",
                statement,
                &[
                    "2024-01-15 CLT",
                    "2024-01-15 12:00:00 CLT",
                    "2024-01-15 12:00:00 CLT",
                ],
            ),
            wire::describe_portal_frame("clt_values"),
            wire::execute_frame("clt_values"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("default-dynamic-abbreviation-text-parameters", steps);

    // Assert
    assert_eq!(batches.len(), 4);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameters = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25, 25]
    );
    assert_eq!(wire::error_code(&batches[3]), None);
    let description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("portal RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1082, 1114, 1184]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![
            Some("2024-01-15".to_owned()),
            Some("2024-01-15 12:00:00".to_owned()),
            Some("2024-01-15 15:00:00+00".to_owned()),
        ]]
    );
}

#[test]
fn should_recover_prepared_timestamp_after_dynamic_abbreviation_input_error() {
    // Arrange
    let statement = "default_dynamic_abbreviation_recovery";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1::TIMESTAMPTZ AS timestamp_value",
            &[25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("invalid_clt", statement, &["2024-02-30 12:00:00 CLT"]),
            wire::execute_frame("invalid_clt"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("valid_clt", statement, &["2024-01-15 12:00:00 CLT"]),
            wire::describe_portal_frame("valid_clt"),
            wire::execute_frame("valid_clt"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("default-dynamic-abbreviation-parameter-recovery", steps);

    // Assert
    assert_eq!(batches.len(), 5);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::error_code(&batches[2]), None);
    let parameters = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![25]);
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("22008"));
    assert!(!batches[3]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(batches[3].last().map(|(tag, _)| *tag), Some(b'Z'));
    assert_eq!(wire::error_code(&batches[4]), None);
    let description = batches[4]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("recovered portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1184
    );
    assert_eq!(
        wire::data_rows(&batches[4]),
        vec![vec![Some("2024-01-15 15:00:00+00".to_owned())]]
    );
}
