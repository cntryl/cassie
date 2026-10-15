//! Scalar-subquery width errors for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_42601_given_a_multicolumn_scalar_subquery_when_it_is_parsed() {
    // Arrange
    let batches = run_script(
        "scalar-subquery-width-error",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE scalar_width_rows (id INT4 PRIMARY KEY, first_value INT4, second_value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO scalar_width_rows VALUES (1, 10, 20)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT first_value, second_value FROM scalar_width_rows WHERE id = 1",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT first_value FROM scalar_width_rows WHERE id = 1) AS scalar_value",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT first_value, second_value FROM scalar_width_rows WHERE id = 1) AS scalar_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let table_control = &batches[2];
    let one_column_scalar_control = &batches[3];
    let multicolumn_scalar = &batches[4];
    let error_fields = multicolumn_scalar
        .iter()
        .find(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1));

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar-subquery setup ErrorResponses: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(table_control), None);
    assert_eq!(
        wire::data_rows(table_control),
        vec![vec![Some("10".to_owned()), Some("20".to_owned())]]
    );
    assert_eq!(wire::error_code(one_column_scalar_control), None);
    let scalar_description = one_column_scalar_control
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("one-column scalar-subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&scalar_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(one_column_scalar_control),
        vec![vec![Some("10".to_owned())]]
    );
    assert_eq!(
        wire::error_code(multicolumn_scalar).as_deref(),
        Some("42601"),
        "multicolumn scalar-subquery ErrorResponse: {error_fields:?}"
    );
    assert!(error_fields.as_ref().is_some_and(|fields| {
        fields
            .iter()
            .any(|(kind, value)| *kind == 'M' && value == "subquery must return only one column")
    }));
}
