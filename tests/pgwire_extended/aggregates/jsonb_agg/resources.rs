//! Resource and cancellation contracts for `jsonb_agg` (PostgreSQL issue #811).

use crate::support_collection_aggregate_resources as support;

#[test]
fn should_reject_jsonb_agg_growth_given_a_memory_budget_when_more_values_arrive() {
    // Arrange
    let control_sql = "SELECT length(jsonb_agg(repeat('x', 2))::text) \
                       FROM (VALUES (1),(2),(3)) AS input(n)";
    let candidate_sql = support::wide_input_query("length(jsonb_agg(repeat('x', 512))::text)");
    let wide_control_sql = support::wide_input_query("count(*)");

    // Act
    let case = support::run_memory_budget_case(
        "issue-811-jsonb-agg-budget",
        control_sql,
        &wide_control_sql,
        &candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "18", &case.recovery, "46");
    support::assert_control_result(&case.wide_control, "100000");
    support::assert_rejected_without_partial_rows(&case.candidate, "54000");
}

#[test]
fn should_cancel_jsonb_agg_without_partial_output_given_retained_values_when_cancel_request_arrives(
) {
    // Arrange
    let control_sql = "SELECT length(jsonb_agg(repeat('x', 2))::text) \
                       FROM (VALUES (1),(2),(3)) AS input(n)";
    let candidate_sql = support::wide_input_query("length(jsonb_agg(repeat('x', 512))::text)");

    // Act
    let case =
        support::run_cancellation_case("issue-811-jsonb-agg-cancel", control_sql, &candidate_sql);

    // Assert
    support::assert_control_and_recovery(&case.control, "18", &case.recovery, "47");
    support::assert_rejected_without_partial_rows(&case.cancelled, "57014");
}
