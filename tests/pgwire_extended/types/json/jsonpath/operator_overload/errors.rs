//! PostgreSQL 18.6 `JSONB` operator overload and `JSONPath` expression residuals for #809.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn assert_error_without_result_rows(frames: &fixture::Frames, expected_state: &str) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(expected_state));
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    assert!(!frames.iter().any(|(tag, _)| *tag == b'T'));
}

fn assert_recovered_select(frames: &fixture::Frames) {
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("42".to_owned())]]);
}

#[test]
fn should_report_malformed_jsonpath_text_sqlstate() {
    // Arrange
    let malformed_jsonpath = fixture::execute_cycle("SELECT '$.a ? ('::JSONPATH", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-malformed-jsonpath-cast",
        &[],
        vec![malformed_jsonpath, recovery],
    );

    // Assert
    assert_error_without_result_rows(&batches[0], "42601");
    assert_recovered_select(&batches[1]);
}

include!("semantics.rs");

include!("../../operator_overload/errors.rs");

include!("../../operator_overload/nulls.rs");

include!("../../operator_overload/semantics.rs");
