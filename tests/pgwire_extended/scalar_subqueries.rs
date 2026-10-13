//! PostgreSQL Parse, Describe, Bind and Execute behavior for #762 subqueries.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn setup() -> Step {
    Step::ThroughReady(vec![wire::simple_query_frame(
        "CREATE TABLE scalar_wire_values (id INT, value INT); \
         INSERT INTO scalar_wire_values (id, value) VALUES (1, 100), (2, 200)",
    )])
}

#[test]
fn should_execute_scalar_subqueries_in_simple_query_protocol() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-simple-query",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT 17 AS scalar_value) AS scalar_value",
            )]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("17".into())]]);
}

#[test]
fn should_describe_scalar_subquery_parameter_metadata() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-describe",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let described = &batches[2];

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
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23]);
    let description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].name, "scalar_value");
    assert_eq!(columns[0].type_oid, 23);
}

#[test]
fn should_bind_text_parameters_to_scalar_subqueries() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-text-bind",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("scalar_text", "scalar_subquery", &["1"]),
                wire::execute_frame("scalar_text"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let result = &batches[3];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("100".into())]]);
}

#[test]
fn should_preserve_column_metadata_for_empty_scalar_subquery_results() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-empty-result-metadata",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("scalar_empty", "scalar_subquery", &["99"]),
                wire::describe_portal_frame("scalar_empty"),
                wire::execute_frame("scalar_empty"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let result = &batches[3];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![None]]);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("empty scalar RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
}

#[test]
fn should_bind_binary_parameters_to_scalar_subqueries() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-binary-bind",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "scalar_binary",
                    "scalar_subquery",
                    &[1],
                    &[Some(&1_i32.to_be_bytes())],
                    &[1],
                ),
                wire::describe_portal_frame("scalar_binary"),
                wire::execute_frame("scalar_binary"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let result = &batches[3];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    let binary_description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary RowDescription");
    let column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(column.type_oid, 23);
    assert_eq!(column.format_code, 1);
    let data_row = result
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary DataRow");
    assert_eq!(
        i16::from_be_bytes(data_row.1[0..2].try_into().expect("field count")),
        1
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[2..6].try_into().expect("field length")),
        4
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[6..10].try_into().expect("binary INT4")),
        100
    );
}
