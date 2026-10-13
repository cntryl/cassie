//! PostgreSQL 18 pattern and text-search protocol contracts tracked by #799.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_ilike_parameter_types() {
    // Arrange
    let batches = run_script(
        "issue-799-ilike-parameter-description",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "ilike_parameters",
                "SELECT $1 ILIKE $2 AS matched",
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("ilike_parameters"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 16);
}

#[test]
fn should_bind_text_values_to_ilike_operands() {
    // Arrange
    let batches = run_script(
        "issue-799-ilike-parameter-bind",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "ilike_parameters",
                "SELECT $1 ILIKE $2 AS matched",
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("ilike_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("ilike_portal", "ilike_parameters", &["Alice", "a%"]),
                wire::execute_frame("ilike_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let result = &batches[2];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_describe_regex_text_parameters_with_boolean_result_metadata() {
    // Arrange
    let batches = run_script(
        "issue-799-regex-parameter-description",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regex_parameters",
                "SELECT $1 ~ $2 AS matched",
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regex_parameters"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 16);
}

#[test]
fn should_return_postgres_invalid_regex_sqlstate_on_the_wire() {
    // Arrange
    let batches = run_script(
        "issue-799-invalid-regex-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'text' ~ '('",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("2201B"));
}

#[test]
fn should_describe_tsvector_results_with_the_postgres_oid() {
    // Arrange
    let batches = run_script(
        "issue-799-tsvector-type-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "tsvector_parameter",
                "SELECT to_tsvector('simple', $1) AS value",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("tsvector_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("tsvector RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 3614);
}

#[test]
fn should_describe_tsquery_results_with_the_postgres_oid() {
    // Arrange
    let batches = run_script(
        "issue-799-tsquery-type-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "tsquery_parameter",
                "SELECT to_tsquery('simple', $1) AS value",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("tsquery_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("tsquery RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 3615);
}

#[test]
fn should_bind_text_patterns_to_tsquery_constructors() {
    // Arrange
    let batches = run_script(
        "issue-799-tsquery-parameter-bind",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "tsquery_parameter",
                "SELECT to_tsquery('simple', $1) AS value",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("tsquery_parameter"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("tsquery_portal", "tsquery_parameter", &["blue & sky"]),
                wire::execute_frame("tsquery_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let columns = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("tsquery RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 3615);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("'blue' & 'sky'".to_owned())]]
    );
}
