//! Constant planning belongs to Bind; Parse/Describe expose only semantics and metadata.
use crate::support_bind_planning as planning;
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_division_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [("SELECT 1/0", "22012"), ("SELECT 1/(1-1)", "22012")];
    // Act
    let batches = planning::failures("bind-division", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_integer_arithmetic_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [
        ("SELECT 2147483647::INT+1", "22003"),
        ("SELECT 32767::SMALLINT+1::SMALLINT", "22003"),
    ];
    // Act
    let batches = planning::failures("bind-integer-arithmetic", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_integer_negation_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [("SELECT -(-9223372036854775808::BIGINT)", "22003")];
    // Act
    let batches = planning::failures("bind-integer-negation", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_numeric_narrowing_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [
        ("SELECT 9223372036854775808::BIGINT", "22003"),
        ("SELECT 2147483648::INT", "22003"),
        ("SELECT 32768::SMALLINT", "22003"),
    ];
    // Act
    let batches = planning::failures("bind-numeric-narrowing", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_typed_integer_inputs_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [
        ("SELECT CAST('bad' AS TEXT)::INT", "22P02"),
        ("SELECT CAST('40000' AS TEXT)::SMALLINT", "22003"),
    ];
    // Act
    let batches = planning::failures("bind-typed-integer-inputs", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_typed_boolean_inputs_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [("SELECT CAST('bad' AS TEXT)::BOOLEAN", "22P02")];
    // Act
    let batches = planning::failures("bind-typed-boolean-inputs", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_typed_float_inputs_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [
        ("SELECT CAST('bad' AS TEXT)::FLOAT8", "22P02"),
        ("SELECT CAST('1e9999' AS TEXT)::FLOAT8", "22003"),
    ];
    // Act
    let batches = planning::failures("bind-typed-float-inputs", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_selected_case_arms_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [("SELECT CASE WHEN TRUE THEN 1/0 ELSE 7 END", "22012")];
    // Act
    let batches = planning::failures("bind-selected-case-arms", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_reached_coalesce_arguments_given_bind_flush_when_planning_precedes_portal_publication(
) {
    // Arrange
    let cases = [("SELECT COALESCE(NULL::INT, 1/0)", "22012")];
    // Act
    let batches = planning::failures("bind-reached-coalesce-arguments", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_values_outputs_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [
        ("VALUES (1/0)", "22012"),
        ("SELECT v.n FROM (VALUES (1/0)) v(n)", "22012"),
    ];
    // Act
    let batches = planning::failures("bind-values-outputs", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_referenced_ctes_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [("WITH c AS (SELECT 1/0 AS n) SELECT n FROM c", "22012")];
    // Act
    let batches = planning::failures("bind-referenced-ctes", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_ordering_keys_given_bind_flush_when_planning_precedes_portal_publication() {
    // Arrange
    let cases = [("SELECT 7 ORDER BY 1/0", "22012")];
    // Act
    let batches = planning::failures("bind-ordering-keys", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_false_filtered_outputs_given_bind_flush_when_planning_precedes_portal_publication()
{
    // Arrange
    let cases = [
        ("SELECT 1/0 WHERE FALSE", "22012"),
        ("SELECT CAST('bad' AS TEXT)::INT WHERE FALSE", "22P02"),
    ];
    // Act
    let batches = planning::failures("bind-false-filtered-outputs", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_prune_dead_case_arms_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "SELECT (CASE WHEN FALSE THEN 1/0 ELSE 7 END)::INT";
    // Act
    let batches = planning::pruned("bind-dead-case-arms", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_later_coalesce_arguments_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "SELECT COALESCE(7, 1/0)::INT";
    // Act
    let batches = planning::pruned("bind-later-coalesce-arguments", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_false_boolean_operands_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "SELECT FALSE AND (1/0=0)";
    // Act
    let batches = planning::pruned("bind-false-boolean-operands", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "f");
}

#[test]
fn should_prune_true_boolean_operands_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "SELECT TRUE OR (1/0=0)";
    // Act
    let batches = planning::pruned("bind-true-boolean-operands", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_prune_exists_targets_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "SELECT EXISTS(SELECT 1/0)";
    // Act
    let batches = planning::pruned("bind-exists-targets", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_prune_unused_values_fields_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "SELECT 1 FROM (VALUES (1/0)) v(n)";
    // Act
    let batches = planning::pruned("bind-unused-values-fields", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "1");
}

#[test]
fn should_prune_unused_ctes_given_bind_planning_when_constants_decide_reachability() {
    // Arrange
    let sql = "WITH unused AS (SELECT 1/0 AS n) SELECT 7";
    // Act
    let batches = planning::pruned("bind-unused-ctes", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_leave_failed_portals_absent_given_bind_planning_errors_when_sync_recovers() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("bad", "SELECT 1/0")),
        Step::BindFlush(wire::bind_frame_with_formats(
            "failed",
            "bad",
            &[],
            &[],
            &[],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![
            wire::describe_portal_frame("failed"),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bind-failed-portal-absent", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22012"));
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("34000"));
}

#[test]
fn should_recheck_constant_errors_given_repeated_bind_attempts_when_prepared_metadata_is_cached() {
    // Arrange
    let mut steps = vec![Step::ParseFlush(wire::parse_frame("bad", "SELECT 1/0"))];
    for _ in 0..3 {
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame("bad"),
            wire::sync_frame(),
        ]));
        steps.push(Step::BindFlush(wire::bind_frame_with_formats(
            "failed",
            "bad",
            &[],
            &[],
            &[],
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }
    // Act
    let batches = run_script("bind-repeated-plan-error", steps);
    // Assert
    for batch in batches[1..].as_chunks::<3>().0 {
        fixture::assert_success(&batch[0]);
        assert_eq!(wire::error_code(&batch[1]).as_deref(), Some("22012"));
        assert_eq!(batch[1].iter().map(|f| f.0).collect::<Vec<_>>(), vec![b'E']);
        assert_eq!(batch[2].iter().map(|f| f.0).collect::<Vec<_>>(), vec![b'Z']);
    }
}

#[test]
fn should_reuse_failed_portal_names_given_valid_statements_when_sync_has_recovered() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("bad", "SELECT 1/0")),
        Step::ParseFlush(wire::parse_frame("good", "SELECT 7::INT")),
        Step::BindFlush(wire::bind_frame_with_formats("out", "bad", &[], &[], &[1])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::BindFlush(wire::bind_frame_with_formats("out", "good", &[], &[], &[1])),
        Step::ThroughReady(vec![wire::execute_frame("out"), wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bind-failed-name-reuse", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22012"));
    assert_eq!(batches[4][0].0, b'2');
    fixture::assert_success(&batches[5]);
    assert_eq!(
        fixture::data_row_payloads(&batches[5]),
        vec![fixture::one_field_row(&7_i32.to_be_bytes())]
    );
}

#[test]
fn should_abort_explicit_transactions_given_bind_planning_errors_when_sync_reports_status() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]),
        Step::ParseFlush(wire::parse_frame("bad", "SELECT 1/0")),
        Step::BindFlush(wire::bind_frame_with_formats("out", "bad", &[], &[], &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
        Step::ThroughReady(vec![wire::simple_query_frame("ROLLBACK")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bind-explicit-transaction-failure", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22012"));
    assert_eq!(batches[3].last().expect("Ready").1, b"E");
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("25P02"));
    fixture::assert_success(&batches[5]);
    fixture::assert_success(&batches[6]);
    assert_eq!(wire::data_rows(&batches[6]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_roll_back_implicit_writes_given_bind_planning_errors_when_the_cycle_aborts() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE bind_items (n INT)",
        )]),
        Step::ParseFlush(wire::parse_frame(
            "write",
            "INSERT INTO bind_items VALUES (7)",
        )),
        Step::ParseFlush(wire::parse_frame("bad", "SELECT 1/0")),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("insert", "write", &[], &[], &[]),
            wire::execute_frame("insert"),
            wire::bind_frame_with_formats("fail", "bad", &[], &[], &[]),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT count(*) FROM bind_items",
        )]),
    ];
    // Act
    let batches = run_script("bind-implicit-write-rollback", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("22012"));
    assert_eq!(batches[3].iter().filter(|f| f.0 == b'2').count(), 1);
    fixture::assert_success(&batches[4]);
    assert_eq!(wire::data_rows(&batches[4]), vec![vec![Some("0".into())]]);
}

fn dml_bind(label: &str, sql: &str) -> Vec<fixture::Frames> {
    run_script(
        label,
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE bind_items (n INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO bind_items VALUES (7)",
            )]),
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::BindFlush(wire::bind_frame_with_formats("out", "p", &[], &[], &[])),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT n FROM bind_items")]),
        ],
    )
}

fn assert_unchanged(batches: &[fixture::Frames]) {
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(batches[2][0].0, b'1');
    fixture::assert_success(&batches[3]);
    fixture::assert_success(&batches[5]);
    fixture::assert_success(&batches[6]);
    assert_eq!(wire::data_rows(&batches[6]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_defer_insert_writes_given_successful_bind_when_execute_has_not_been_sent() {
    // Arrange
    let sql = "INSERT INTO bind_items VALUES (9)";
    // Act
    let batches = dml_bind("bind-insert-no-execute", sql);
    // Assert
    assert_eq!(
        batches[4].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'2']
    );
    assert_unchanged(&batches);
}

#[test]
fn should_defer_update_writes_given_successful_bind_when_execute_has_not_been_sent() {
    // Arrange
    let sql = "UPDATE bind_items SET n=9";
    // Act
    let batches = dml_bind("bind-update-no-execute", sql);
    // Assert
    assert_eq!(
        batches[4].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'2']
    );
    assert_unchanged(&batches);
}

#[test]
fn should_defer_delete_writes_given_successful_bind_when_execute_has_not_been_sent() {
    // Arrange
    let sql = "DELETE FROM bind_items";
    // Act
    let batches = dml_bind("bind-delete-no-execute", sql);
    // Assert
    assert_eq!(
        batches[4].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'2']
    );
    assert_unchanged(&batches);
}

#[test]
fn should_reject_insert_constants_given_bind_planning_when_no_row_has_been_written() {
    // Arrange
    let sql = "INSERT INTO bind_items VALUES (1/0)";
    // Act
    let batches = dml_bind("bind-insert-plan-error", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22012"));
    assert_unchanged(&batches);
}

#[test]
fn should_reject_update_constants_given_bind_planning_when_no_row_has_been_written() {
    // Arrange
    let sql = "UPDATE bind_items SET n=1/0 WHERE FALSE";
    // Act
    let batches = dml_bind("bind-update-plan-error", sql);
    // Assert
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22012"));
    assert_unchanged(&batches);
}

#[test]
fn should_prefer_text_receiver_errors_given_invalid_parameters_when_bind_also_has_constant_errors()
{
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT $1::INT, 1/0",
            &[23],
        )),
        Step::BindFlush(wire::bind_frame_with_formats(
            "out",
            "p",
            &[],
            &[Some(b"bad")],
            &[],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bind-text-error-priority", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22P02"));
    assert_eq!(
        batches[1].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'E']
    );
}

#[test]
fn should_prefer_binary_receiver_errors_given_invalid_parameters_when_bind_also_has_constant_errors(
) {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT $1::INT, 1/0",
            &[23],
        )),
        Step::BindFlush(wire::bind_frame_with_formats(
            "out",
            "p",
            &[1],
            &[Some(b"x")],
            &[],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bind-binary-error-priority", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("08P01"));
    assert_eq!(
        batches[1].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'E']
    );
}

#[test]
fn should_prefer_parameter_count_errors_given_missing_values_when_bind_also_has_constant_errors() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT $1::INT, 1/0",
            &[23],
        )),
        Step::BindFlush(wire::bind_frame_with_formats("out", "p", &[], &[], &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bind-count-error-priority", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("08P01"));
    assert_eq!(
        batches[1].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'E']
    );
}

#[test]
fn should_defer_explain_planning_given_bind_when_the_utility_statement_has_not_executed() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("p", "EXPLAIN SELECT 1/0")),
        Step::BindFlush(wire::bind_frame_with_formats("out", "p", &[], &[], &[])),
        Step::ThroughReady(vec![wire::execute_frame("out"), wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bind-explain-deferred-planning", steps);
    // Assert
    assert_eq!(
        batches[1].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'2']
    );
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22012"));
}
