//! Derived VALUES descriptors with inferred mixed NULL column types.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_describe_derived_values_default_columns_with_mixed_nulls() {
    // Arrange
    let sql = "SELECT * FROM (VALUES (NULL, 'alpha'), (2, NULL)) AS v ORDER BY column1 NULLS FIRST";
    let batches = run_script(
        "issue-800-values-default-column-descriptor",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("derived VALUES RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["column1", "column2"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        [(23, 4, -1), (25, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![None, Some("alpha".to_owned())],
            vec![Some("2".to_owned()), None],
        ]
    );
}
