//! Scalar-subquery cardinality error contract for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_21000_given_a_multirow_scalar_subquery_when_its_value_is_requested() {
    // Arrange
    let batches = run_script(
        "scalar-subquery-single-row-value",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE scalar_cardinality_rows (id INT4 PRIMARY KEY, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO scalar_cardinality_rows VALUES (1, 10), (2, 20)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT value FROM scalar_cardinality_rows WHERE id = 1",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT value FROM scalar_cardinality_rows WHERE id = 1) AS scalar_value",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT value FROM scalar_cardinality_rows) AS scalar_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let table_control = &batches[2];
    let single_row_scalar = &batches[3];
    let multirow_scalar = &batches[4];
    let error_fields = multirow_scalar
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
        vec![vec![Some("10".to_owned())]]
    );
    assert_eq!(
        wire::error_code(single_row_scalar),
        None,
        "single-row scalar-subquery ErrorResponse: {:?}",
        single_row_scalar
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let description = single_row_scalar
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("single-row scalar-subquery RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(single_row_scalar),
        vec![vec![Some("10".to_owned())]]
    );
    assert_eq!(
        wire::error_code(multirow_scalar).as_deref(),
        Some("21000"),
        "multirow scalar-subquery ErrorResponse: {error_fields:?}"
    );
    assert!(error_fields.as_ref().is_some_and(|fields| {
        fields.iter().any(|(kind, value)| {
            *kind == 'M'
                && value == "more than one row returned by a subquery used as an expression"
        })
    }));
}
