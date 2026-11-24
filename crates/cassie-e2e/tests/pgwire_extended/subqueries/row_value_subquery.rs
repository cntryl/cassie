//! Row-valued subquery comparisons for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_compare_a_row_constructor_with_a_one_row_subquery() {
    // Arrange
    let batches = run_script(
        "issue-762-row-value-subquery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_row_values \
                   (id INT4 PRIMARY KEY, left_value INT4, right_value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_row_values VALUES (1, 1, 2), (2, 3, 4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT ROW(1::INT4, 2::INT4) = \
                   (SELECT left_value, right_value FROM issue_762_row_values WHERE id = 1) \
                   AS matches",
            )]),
        ],
    );

    // Act
    let create = &batches[0];
    let insert = &batches[1];
    let comparison = &batches[2];
    let error_fields = comparison
        .iter()
        .find(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1));

    // Assert
    assert_eq!(wire::error_code(create), None);
    assert_eq!(wire::error_code(insert), None);
    assert_eq!(
        wire::error_code(comparison),
        None,
        "row-valued subquery ErrorResponse: {error_fields:?}"
    );
    let description = comparison
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-valued subquery Boolean RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(
        wire::data_rows(comparison),
        vec![vec![Some("t".to_owned())]]
    );
}
