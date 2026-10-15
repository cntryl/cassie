use super::*;

#[test]
fn should_report_int4range_maximum_inclusive_bound_overflow() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT int4range(2147483647, 2147483647, '[]')",
    )])];

    // Act
    let batches = run_script("issue-905-int4range-maximum-inclusive-bound", steps);

    // Assert
    let result = &batches[0];
    let error_fields = result
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("inclusive INT32_MAX bound raises a range canonicalization error");
    assert_eq!(wire::error_code(result), Some("22003".to_owned()));
    assert!(
        error_fields
            .iter()
            .any(|(kind, message)| *kind == 'M' && message == "integer out of range"),
        "expected the canonical upper-bound successor overflow; fields: {error_fields:?}"
    );
}
