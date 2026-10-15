//! Scalar subqueries in DELETE predicates for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_delete_rows_selected_by_a_scalar_subquery_predicate() {
    // Arrange
    let batches = run_script(
        "issue-762-delete-scalar-subquery-predicate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_delete_target (id INT4 PRIMARY KEY, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_delete_source (id INT4, match_key INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_delete_target VALUES (1, 10), (2, 20), (3, 30)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_delete_source VALUES (2, 99)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "DELETE FROM issue_762_delete_target AS target \
                 WHERE target.id = (SELECT source.id \
                                    FROM issue_762_delete_source AS source \
                                    WHERE source.match_key = 99) \
                 RETURNING target.id",
            )]),
        ],
    );

    // Act
    let setup = &batches[..4];
    let deletion = &batches[4];
    let setup_errors = setup
        .iter()
        .flat_map(|batch| batch.iter().filter(|frame| frame.0 == b'E'))
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    let delete_errors = deletion
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();

    // Assert
    assert!(
        setup_errors.is_empty(),
        "DELETE setup errors: {setup_errors:?}"
    );
    assert_eq!(
        wire::error_code(deletion),
        None,
        "DELETE scalar-subquery ErrorResponse: {delete_errors:?}"
    );
    let description = deletion
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("DELETE RETURNING RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(wire::data_rows(deletion), vec![vec![Some("2".to_owned())]]);
}
