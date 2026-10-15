//! PostgreSQL 18 Boolean aggregate values and result metadata for #805.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_boolean_aggregate_truth_table() {
    // Arrange
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_805_boolean_truth (bucket TEXT, flag BOOLEAN)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_805_boolean_truth VALUES \
             ('all-null', NULL), ('all-null', NULL), \
             ('all-true', TRUE), ('all-true', TRUE), \
             ('false', FALSE), ('false', FALSE), \
             ('mixed', TRUE), ('mixed', FALSE), ('mixed', NULL)",
        )]),
    ];
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT bucket, BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) \
         FROM issue_805_boolean_truth GROUP BY bucket ORDER BY bucket",
    )]));

    // Act
    let batches = run_script("issue-805-boolean-aggregate-values", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let result = &batches[2];
    let error = result
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload));
    assert_eq!(wire::error_code(result), None, "ErrorResponse fields: {error:?}");
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("Boolean aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("all-null".to_owned()), None, None, None],
            vec![
                Some("all-true".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned()),
            ],
            vec![
                Some("false".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
            ],
            vec![
                Some("mixed".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned()),
            ],
        ]
    );
}
