//! Scalar subqueries in DML RETURNING expressions for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_correlated_scalar_subquery_values_from_update() {
    // Arrange
    let batches = run_script(
        "issue-762-update-returning-scalar-subquery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_returning_target (id INT4 PRIMARY KEY, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_returning_lookup (id INT4, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_returning_target VALUES (1, 10)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_returning_lookup VALUES (1, 200)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "UPDATE issue_762_returning_target AS target SET value = 20 \
                 WHERE target.id = 1 \
                 RETURNING target.id, \
                   (SELECT lookup.value FROM issue_762_returning_lookup AS lookup \
                    WHERE lookup.id = target.id) AS lookup_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..4];
    let result = &batches[4];

    // Assert
    for (index, batch) in setup.iter().enumerate() {
        let errors = batch
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>();
        assert_eq!(
            wire::error_code(batch),
            None,
            "RETURNING setup statement {index} ErrorResponse: {errors:?}"
        );
    }
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(result),
        None,
        "UPDATE RETURNING scalar-subquery ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("UPDATE RETURNING RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1".to_owned()), Some("200".to_owned())]]
    );
}
