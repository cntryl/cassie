//! Scalar subqueries in LIMIT and OFFSET expression positions for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_apply_scalar_subquery_to_limit_expression() {
    // Arrange
    let batches = run_script(
        "issue-762-limit-subquery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_limit_values (id INT4 PRIMARY KEY)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_limit_values VALUES (1), (2), (3), (4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_762_limit_values ORDER BY id \
                 LIMIT (SELECT 2::INT4)",
            )]),
        ],
    );

    // Act
    let setup = &batches[0];
    let insert = &batches[1];
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(setup), None);
    assert_eq!(wire::error_code(insert), None);
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("LIMIT/OFFSET subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
}

#[test]
fn should_apply_scalar_subquery_to_offset_expression() {
    // Arrange
    let batches = run_script(
        "issue-762-offset-subquery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_offset_values (id INT4 PRIMARY KEY)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_offset_values VALUES (1), (2), (3), (4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_762_offset_values ORDER BY id \
                 OFFSET (SELECT 2::INT4)",
            )]),
        ],
    );

    // Act
    let setup = &batches[0];
    let insert = &batches[1];
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(setup), None);
    assert_eq!(wire::error_code(insert), None);
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("OFFSET subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("3".to_owned())], vec![Some("4".to_owned())]]
    );
}
