//! PostgreSQL row-constructor ordering behavior when a NULL field is reached.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_stop_row_greater_ordering_at_the_first_decisive_field() {
    // Arrange
    let sql = "SELECT \
        ROW(2::INT4, NULL::INT4) > ROW(1::INT4, 99::INT4) AS decisive_prefix, \
        ROW(1::INT4, NULL::INT4) >= ROW(1::INT4, 2::INT4) AS unresolved_null";
    let batches = run_script(
        "issue-800-row-ordering-greater-null-stop-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-ordering Boolean RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["decisive_prefix", "unresolved_null"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("t".to_owned()), None]]
    );
}
