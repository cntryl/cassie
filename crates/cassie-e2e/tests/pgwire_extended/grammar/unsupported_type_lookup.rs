//! Missing PostgreSQL type-name lookup errors for #796.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_report_undefined_type_names_with_42704_for_cast_forms() {
    // Arrange
    let cases = [
        "SELECT 1::issue_796_missing_type",
        "SELECT CAST(1 AS issue_796_missing_type)",
    ];
    let batches = run_script(
        "issue-796-undefined-type-name-casts",
        cases
            .iter()
            .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
            .collect(),
    );

    // Act
    let errors = batches
        .iter()
        .map(|batch| {
            batch
                .iter()
                .find(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
        })
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(batches.len(), cases.len());
    for (sql, fields) in cases.iter().zip(errors) {
        let fields = fields.unwrap_or_else(|| panic!("{sql}: expected ErrorResponse"));
        assert!(
            fields
                .iter()
                .any(|(kind, value)| *kind == 'C' && value == "42704"),
            "{sql}: PostgreSQL 18.6 reports undefined_object 42704, got {fields:?}"
        );
        assert!(
            fields.iter().any(|(kind, value)| {
                *kind == 'M' && value == "type \"issue_796_missing_type\" does not exist"
            }),
            "{sql}: expected missing type-name diagnostic, got {fields:?}"
        );
    }
}
