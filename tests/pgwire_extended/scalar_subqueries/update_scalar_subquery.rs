//! Scalar subqueries in UPDATE assignment expressions for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_apply_a_one_row_scalar_subquery_in_an_update_assignment() {
    // Arrange
    let batches = run_script(
        "issue-762-update-scalar-subquery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_update_target (id INT4 PRIMARY KEY, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_update_source (id INT4, value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_update_target VALUES (1, 10)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_update_source VALUES (1, 200)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "UPDATE issue_762_update_target AS target \
                 SET value = (SELECT source.value FROM issue_762_update_source AS source \
                              WHERE source.id = target.id) \
                 WHERE target.id = 1 RETURNING target.id, target.value",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT value FROM issue_762_update_target WHERE id = 1",
            )]),
        ],
    );

    // Act
    let create_target = &batches[0];
    let create_source = &batches[1];
    let insert_target = &batches[2];
    let insert_source = &batches[3];
    let update = &batches[4];
    let readback = &batches[5];
    let error_fields = update
        .iter()
        .find(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1));

    // Assert
    assert_eq!(wire::error_code(create_target), None);
    assert_eq!(wire::error_code(create_source), None);
    assert_eq!(wire::error_code(insert_target), None);
    assert_eq!(wire::error_code(insert_source), None);
    assert_eq!(
        wire::error_code(update),
        None,
        "UPDATE scalar-subquery ErrorResponse: {error_fields:?}"
    );
    let update_description = update
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("UPDATE RETURNING RowDescription");
    assert_eq!(
        wire::parse_row_description(&update_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23]
    );
    assert_eq!(
        wire::data_rows(update),
        vec![vec![Some("1".to_owned()), Some("200".to_owned())]]
    );
    assert_eq!(wire::error_code(readback), None);
    let readback_description = readback
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("updated value RowDescription");
    assert_eq!(
        wire::parse_row_description(&readback_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(readback),
        vec![vec![Some("200".to_owned())]]
    );
}
