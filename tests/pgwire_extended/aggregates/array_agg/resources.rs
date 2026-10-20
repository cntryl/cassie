//! Resource and cancellation contracts for `array_agg` (PostgreSQL issue #811).

use crate::support_collection_aggregate_resources as support;

#[test]
fn should_reject_array_agg_growth_given_a_memory_budget_when_more_values_arrive() {
    // Arrange
    // 64 values of 512 bytes exceed the 4 KiB budget without a scale fixture.
    let setup_sql = support::integer_table_fixture_setup("issue_811_array_agg_rows", 64);
    let control_sql = "SELECT cardinality(array_agg(repeat('x', 2))) \
                       FROM issue_811_array_agg_rows WHERE n <= 3";
    let candidate_sql = "SELECT cardinality(array_agg(repeat('x', 512))) \
                         FROM issue_811_array_agg_rows";
    let wide_control_sql = "SELECT count(*) FROM issue_811_array_agg_rows";

    // Act
    let case = support::run_memory_budget_case_with_setup(
        "issue-811-array-agg-budget",
        &setup_sql,
        control_sql,
        wide_control_sql,
        candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "3", &case.recovery, "46");
    support::assert_control_result(&case.wide_control, "64");
    support::assert_rejected_without_partial_rows(&case.candidate, "54000");
}

#[test]
fn should_cancel_array_agg_without_partial_output_given_retained_values_when_cancel_request_arrives(
) {
    // Arrange
    let setup_sql = support::integer_table_fixture_setup("issue_811_array_agg_rows", 3);
    let control_sql = "SELECT count(*) FROM issue_811_array_agg_rows WHERE n <= 3";
    let candidate_sql = "SELECT array_agg(n) FROM issue_811_array_agg_rows";

    // Act
    let case = support::run_cancellation_case_with_setup_after_aggregate_input(
        "issue-811-array-agg-cancel",
        &setup_sql,
        control_sql,
        candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "3", &case.recovery, "47");
    support::assert_rejected_without_partial_rows(&case.cancelled, "57014");
}
