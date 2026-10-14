//! PostgreSQL aggregate DISTINCT ordering validation over pgwire for issue #804.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_reject_distinct_aggregate_order_key_outside_arguments_with_42p10() {
    // Arrange
    let batches = run_script(
        "issue-804-distinct-aggregate-order-key-validation",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT string_agg(DISTINCT v, ',' ORDER BY v) \
                 FROM (VALUES ('b', 2), ('a', 1), ('b', 3)) AS input(v, k)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT string_agg(DISTINCT v, ',' ORDER BY k) \
                 FROM (VALUES ('b', 2), ('a', 1)) AS input(v, k)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    let description = valid
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription for valid DISTINCT ordered string_agg");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("a,b".to_owned())]]);

    assert_eq!(wire::error_code(invalid).as_deref(), Some("42P10"));
    let error = invalid
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("aggregate DISTINCT ORDER BY validation ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, message)| message.as_str()),
        Some("in an aggregate with DISTINCT, ORDER BY expressions must appear in argument list")
    );
    assert!(wire::data_rows(invalid).is_empty());
    assert_eq!(invalid.iter().filter(|frame| frame.0 == b'C').count(), 0);

    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
