//! Resource and cancellation contracts for `json_object_agg` (PostgreSQL issue #811).

use crate::support_collection_aggregate_resources as support;

const KEY: &str = "CAST(a.n AS TEXT) || '-' || CAST(b.n AS TEXT) || '-' || \
                  CAST(c.n AS TEXT) || '-' || CAST(d.n AS TEXT) || '-' || CAST(e.n AS TEXT)";

#[test]
fn should_reject_json_object_agg_growth_given_a_memory_budget_when_more_entries_arrive() {
    // Arrange
    let control_sql = "SELECT length(json_object_agg(n::text, repeat('x', 2))::text) \
                       FROM (VALUES (1),(2),(3)) AS input(n)";
    let candidate_sql = support::wide_input_query(&format!(
        "length(json_object_agg({KEY}, repeat('x', 512))::text)"
    ));
    let wide_control_sql = support::wide_input_query("count(*)");

    // Act
    let case = support::run_memory_budget_case(
        "issue-811-json-object-agg-budget",
        control_sql,
        &wide_control_sql,
        &candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "38", &case.recovery, "46");
    support::assert_control_result(&case.wide_control, "100000");
    support::assert_rejected_without_partial_rows(&case.candidate, "54000");
}

#[test]
fn should_cancel_json_object_agg_without_partial_output_given_retained_entries_when_cancel_request_arrives(
) {
    // Arrange
    let control_sql = "SELECT length(json_object_agg(n::text, repeat('x', 2))::text) \
                       FROM (VALUES (1),(2),(3)) AS input(n)";
    let candidate_sql = support::wide_input_query(&format!(
        "length(json_object_agg({KEY}, repeat('x', 512))::text)"
    ));

    // Act
    let case = support::run_cancellation_case(
        "issue-811-json-object-agg-cancel",
        control_sql,
        &candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "38", &case.recovery, "47");
    support::assert_rejected_without_partial_rows(&case.cancelled, "57014");
}
