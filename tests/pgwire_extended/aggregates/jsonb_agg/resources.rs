//! Resource and cancellation contracts for `jsonb_agg` (PostgreSQL issue #811).

use crate::support_collection_aggregate_resources as support;

#[test]
fn should_cancel_jsonb_agg_without_partial_output_given_retained_values_when_cancel_request_arrives(
) {
    // Arrange
    let setup_sql = support::integer_table_fixture_setup("issue_811_jsonb_agg_rows", 3);
    let control_sql = "SELECT count(*) FROM issue_811_jsonb_agg_rows WHERE n <= 3";
    let candidate_sql = "SELECT jsonb_agg(n) FROM issue_811_jsonb_agg_rows";

    // Act
    let case = support::run_cancellation_case_with_setup_after_aggregate_input(
        "issue-811-jsonb-agg-cancel",
        &setup_sql,
        control_sql,
        candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "3", &case.recovery, "47");
    support::assert_rejected_without_partial_rows(&case.cancelled, "57014");
}
