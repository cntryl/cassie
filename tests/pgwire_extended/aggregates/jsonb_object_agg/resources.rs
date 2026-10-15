//! Resource and cancellation contracts for `jsonb_object_agg` (PostgreSQL issue #811).

use crate::support_collection_aggregate_resources as support;

const INPUT_ROW_COUNT: i32 = 50_000;

fn jsonb_object_agg_fixture_setup() -> Vec<String> {
    let mut statements =
        vec!["CREATE TABLE issue_811_jsonb_object_agg_rows (n INT4 PRIMARY KEY)".to_owned()];
    for first in (1..=INPUT_ROW_COUNT).step_by(250) {
        let last = (first + 249).min(INPUT_ROW_COUNT);
        let values = (first..=last)
            .map(|value| format!("({value})"))
            .collect::<Vec<_>>()
            .join(", ");
        statements.push(format!(
            "INSERT INTO issue_811_jsonb_object_agg_rows VALUES {values}"
        ));
    }
    statements
}

#[test]
fn should_reject_jsonb_object_agg_growth_given_a_memory_budget_when_more_entries_arrive() {
    // Arrange
    let setup_sql = jsonb_object_agg_fixture_setup();
    let control_sql = "SELECT length(jsonb_object_agg(n::text, repeat('x', 2))::text) \
                       FROM issue_811_jsonb_object_agg_rows WHERE n <= 3";
    let candidate_sql = "SELECT length(jsonb_object_agg(n::text, repeat('x', 512))::text) \
                         FROM issue_811_jsonb_object_agg_rows";
    let wide_control_sql = "SELECT count(*) FROM issue_811_jsonb_object_agg_rows";

    // Act
    let case = support::run_memory_budget_case_with_setup(
        "issue-811-jsonb-object-agg-budget",
        &setup_sql,
        control_sql,
        wide_control_sql,
        candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "33", &case.recovery, "46");
    support::assert_control_result(&case.wide_control, "50000");
    support::assert_rejected_without_partial_rows(&case.candidate, "54000");
}

#[test]
fn should_cancel_jsonb_object_agg_without_partial_output_given_retained_entries_when_cancel_request_arrives(
) {
    // Arrange
    let setup_sql = jsonb_object_agg_fixture_setup();
    let control_sql = "SELECT length(jsonb_object_agg(n::text, repeat('x', 2))::text) \
                       FROM issue_811_jsonb_object_agg_rows WHERE n <= 3";
    let candidate_sql = "SELECT length(jsonb_object_agg(n::text, repeat('x', 512))::text) \
                         FROM issue_811_jsonb_object_agg_rows";

    // Act
    let case = support::run_cancellation_case_with_setup(
        "issue-811-jsonb-object-agg-cancel",
        &setup_sql,
        control_sql,
        candidate_sql,
    );

    // Assert
    support::assert_control_and_recovery(&case.control, "33", &case.recovery, "47");
    support::assert_rejected_without_partial_rows(&case.cancelled, "57014");
}
