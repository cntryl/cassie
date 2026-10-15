//! `ROWS FROM` NULL padding when one member returns no rows.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_pad_empty_rows_from_member_with_typed_nulls() {
    // Arrange
    let sql = "SELECT * FROM ROWS FROM (generate_series(1, 0), generate_series(10, 12)) \
        WITH ORDINALITY AS expanded(empty_value, populated_value, ordinal)";
    let batches = run_script(
        "issue-808-rows-from-empty-member-padding",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("ROWS FROM RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["empty_value", "populated_value", "ordinal"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![None, Some("10".to_owned()), Some("1".to_owned())],
            vec![None, Some("11".to_owned()), Some("2".to_owned())],
            vec![None, Some("12".to_owned()), Some("3".to_owned())],
        ]
    );
}
