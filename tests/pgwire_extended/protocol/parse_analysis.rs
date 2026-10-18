//! Native PostgreSQL Parse phase, metadata suppression and statement recovery.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_primitive_literal_inputs::INVALID;
use crate::support_type_metadata_contract as fixture;

fn reject(label: &str, cases: &[(String, &'static str)]) -> Vec<fixture::Frames> {
    let mut steps = Vec::new();
    for (sql, _) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            sql,
            &[23, 20],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::bind_frame_with_formats("", "p", &[], &[Some(b"1"), Some(b"2")], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
    }
    steps.push(Step::ParseFlush(wire::parse_frame_with_types(
        "p",
        "SELECT 7::INT",
        &[23, 20],
    )));
    steps.push(Step::ThroughReady(vec![
        wire::describe_statement_frame("p"),
        wire::bind_frame_with_formats("", "p", &[], &[Some(b"1"), Some(b"2")], &[1]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]));
    run_script(label, steps)
}

fn assert_rejected(batches: &[fixture::Frames], cases: &[(String, &'static str)]) {
    for (i, (sql, state)) in cases.iter().enumerate() {
        assert_eq!(
            wire::error_code(&batches[i * 2]).as_deref(),
            Some(*state),
            "{sql}"
        );
        assert_eq!(
            batches[i * 2].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'E'],
            "{sql}"
        );
        assert_eq!(
            batches[i * 2 + 1].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'Z'],
            "{sql}"
        );
    }
    assert_eq!(batches[cases.len() * 2][0].0, b'1');
    let recovered = &batches[cases.len() * 2 + 1];
    fixture::assert_success(recovered);
    assert_eq!(
        fixture::data_row_payloads(recovered),
        vec![fixture::one_field_row(&7_i32.to_be_bytes())]
    );
}

fn invalid_cases(render: impl Fn(&str, &str) -> String) -> Vec<(String, &'static str)> {
    INVALID
        .iter()
        .map(|case| (render(case.input, case.sql_type), case.state))
        .collect()
}

#[test]
fn should_reject_direct_inputs_given_parse_flush_when_trailing_parameter_slots_are_supplied() {
    // Arrange
    let cases = invalid_cases(|input, ty| format!("SELECT '{input}'::{ty}"));
    // Act
    let batches = reject("parse-direct-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_reject_dead_case_inputs_given_parse_flush_when_branches_will_be_pruned() {
    // Arrange
    let cases = invalid_cases(|input, ty| {
        format!("SELECT CASE WHEN FALSE THEN '{input}'::{ty} ELSE NULL::{ty} END")
    });
    // Act
    let batches = reject("parse-case-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_reject_values_inputs_given_parse_flush_when_output_columns_are_unused() {
    // Arrange
    let cases = invalid_cases(|input, ty| format!("SELECT 1 FROM (VALUES ('{input}'::{ty})) v(n)"));
    // Act
    let batches = reject("parse-values-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_reject_coalesce_inputs_given_parse_flush_when_first_argument_is_nonnull() {
    // Arrange
    let cases = invalid_cases(|input, ty| format!("SELECT COALESCE(NULL::{ty}, '{input}'::{ty})"));
    // Act
    let batches = reject("parse-coalesce-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_reject_exists_inputs_given_parse_flush_when_target_columns_are_unused() {
    // Arrange
    let cases = invalid_cases(|input, ty| format!("SELECT EXISTS(SELECT '{input}'::{ty})"));
    // Act
    let batches = reject("parse-exists-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_reject_cte_inputs_given_parse_flush_when_ctes_are_unused() {
    // Arrange
    let cases =
        invalid_cases(|input, ty| format!("WITH unused AS (SELECT '{input}'::{ty}) SELECT 1"));
    // Act
    let batches = reject("parse-cte-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_reject_explain_inputs_given_parse_flush_when_analysis_does_not_execute_the_query() {
    // Arrange
    let cases = invalid_cases(|input, ty| format!("EXPLAIN SELECT '{input}'::{ty}"));
    // Act
    let batches = reject("parse-explain-inputs", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_report_earlier_columns_given_invalid_inputs_when_parse_analyzes_projection_order() {
    // Arrange
    let cases = vec![("SELECT missing, 'bad'::INT".into(), "42703")];
    // Act
    let batches = reject("parse-earlier-column", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_report_earlier_inputs_given_missing_columns_when_parse_analyzes_projection_order() {
    // Arrange
    let cases = vec![("SELECT 'bad'::INT, missing".into(), "22P02")];
    // Act
    let batches = reject("parse-earlier-input", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

#[test]
fn should_report_missing_relations_given_invalid_inputs_when_parse_resolves_sources_first() {
    // Arrange
    let cases = vec![("SELECT 'bad'::INT FROM missing_relation".into(), "42P01")];
    // Act
    let batches = reject("parse-source-first", &cases);
    // Assert
    assert_rejected(&batches, &cases);
}

fn accept(label: &str, queries: &[&str]) -> Vec<fixture::Frames> {
    let mut steps = Vec::new();
    for sql in queries {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            sql,
            &[23, 20],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::close_statement_frame("p"),
            wire::sync_frame(),
        ]));
    }
    run_script(label, steps)
}

fn assert_accepted(batches: &[fixture::Frames], queries: &[&str]) {
    for (i, sql) in queries.iter().enumerate() {
        assert_eq!(
            batches[i * 2].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'1'],
            "{sql}"
        );
        assert_eq!(
            batches[i * 2 + 1].iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![b'3', b'Z'],
            "{sql}"
        );
    }
}

#[test]
fn should_accept_division_by_zero_given_parse_flush_when_constant_evaluation_is_deferred() {
    // Arrange
    let queries = [
        "SELECT 1/0",
        "SELECT 1::BIGINT/0",
        "SELECT CASE WHEN FALSE THEN 1/0 ELSE 7 END",
    ];
    // Act
    let batches = accept("parse-deferred-division", &queries);
    // Assert
    assert_accepted(&batches, &queries);
}

#[test]
fn should_accept_numeric_overflow_given_parse_flush_when_conversion_is_deferred() {
    // Arrange
    let queries = [
        "SELECT 9223372036854775808::BIGINT",
        "SELECT 2147483648::INT",
        "SELECT 32768::SMALLINT",
    ];
    // Act
    let batches = accept("parse-deferred-overflow", &queries);
    // Assert
    assert_accepted(&batches, &queries);
}

#[test]
fn should_accept_typed_text_inputs_given_parse_flush_when_conversion_is_deferred() {
    // Arrange
    let queries = [
        "SELECT CAST('bad' AS TEXT)::INT",
        "SELECT CAST('bad' AS TEXT)::BOOLEAN",
        "SELECT CAST('bad' AS TEXT)::FLOAT8",
    ];
    // Act
    let batches = accept("parse-deferred-typed-text", &queries);
    // Assert
    assert_accepted(&batches, &queries);
}

#[test]
fn should_accept_valid_inputs_given_parse_flush_when_primitive_domains_are_preserved() {
    // Arrange
    let queries = [
        "SELECT '-32768'::SMALLINT",
        "SELECT '2147483647'::INT",
        "SELECT '-9223372036854775808'::BIGINT",
        "SELECT 'NaN'::FLOAT8",
        "SELECT 'of'::BOOLEAN",
        "SELECT NULL::INT",
    ];
    // Act
    let batches = accept("parse-valid-inputs", &queries);
    // Assert
    assert_accepted(&batches, &queries);
}

#[test]
fn should_preserve_raw_values_given_parse_analysis_when_describe_and_execute_rebind() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "p",
            "SELECT v.n FROM (VALUES (7::INT)) v(n)",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::bind_frame_with_formats("", "p", &[], &[], &[1]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "p", &[], &[], &[0]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("parse-raw-values-rebind", steps);
    // Assert
    assert_eq!(batches[0][0].0, b'1');
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&7_i32.to_be_bytes())]
    );
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_recover_unnamed_statements_given_parse_failure_when_sync_resets_the_protocol() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", "SELECT 'bad'::INT")),
        Step::ThroughReady(vec![wire::describe_statement_frame(""), wire::sync_frame()]),
        Step::ParseFlush(wire::parse_frame("", "SELECT 9::INT")),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "", &[], &[], &[0]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("parse-unnamed-recovery", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert_eq!(
        batches[1].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'Z']
    );
    assert_eq!(batches[2][0].0, b'1');
    fixture::assert_success(&batches[3]);
    assert_eq!(wire::data_rows(&batches[3]), vec![vec![Some("9".into())]]);
}

#[test]
fn should_preserve_named_statements_given_another_parse_failure_when_sync_recovers() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("good", "SELECT 11::INT")),
        Step::ParseFlush(wire::parse_frame("bad", "SELECT 'bad'::INT")),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "good", &[], &[], &[1]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("parse-preserve-other-name", steps);
    // Assert
    assert_eq!(batches[0][0].0, b'1');
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22P02"));
    fixture::assert_success(&batches[3]);
    assert_eq!(
        fixture::data_row_payloads(&batches[3]),
        vec![fixture::one_field_row(&11_i32.to_be_bytes())]
    );
}

#[test]
fn should_reuse_closed_statement_names_given_analysis_when_prepared_storage_is_released() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("p", "SELECT 1")),
        Step::ThroughReady(vec![wire::close_statement_frame("p"), wire::sync_frame()]),
        Step::ParseFlush(wire::parse_frame("p", "SELECT 'bad'::INT")),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ParseFlush(wire::parse_frame("p", "SELECT 3")),
    ];
    // Act
    let batches = run_script("parse-closed-name-reuse", steps);
    // Assert
    assert_eq!(batches[0][0].0, b'1');
    assert_eq!(batches[1][0].0, b'3');
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22P02"));
    assert_eq!(batches[4][0].0, b'1');
}

#[test]
fn should_reject_inputs_given_typed_parameters_when_parse_has_completed_slot_admission() {
    // Arrange
    let cases = INVALID
        .iter()
        .map(|case| format!("SELECT $1, '{}'::{}", case.input, case.sql_type))
        .collect::<Vec<_>>();
    let mut steps = Vec::new();
    for sql in &cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            sql,
            &[23, 20],
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }
    // Act
    let batches = run_script("parse-parameterized-inputs", steps);
    // Assert
    for (i, case) in INVALID.iter().enumerate() {
        assert_eq!(
            wire::error_code(&batches[i * 2]).as_deref(),
            Some(case.state)
        );
    }
}

#[test]
fn should_preserve_typed_parameters_given_analysis_when_binary_values_are_bound() {
    // Arrange
    let value = 42_i32.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT $1::INT",
            &[23, 20],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::bind_frame_with_formats(
                "",
                "p",
                &[1],
                &[Some(&value), Some(&1_i64.to_be_bytes())],
                &[1],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("parse-binary-parameters", steps);
    // Assert
    assert_eq!(batches[0][0].0, b'1');
    fixture::assert_success(&batches[1]);
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&value)]
    );
}

#[test]
fn should_preserve_boolean_parameter_inference_given_analysis_when_parse_has_no_declared_types() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("p", "SELECT $1 AND TRUE")),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::bind_frame_with_formats("", "p", &[], &[Some(b"t")], &[0]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("parse-inferred-boolean", steps);
    // Assert
    assert_eq!(batches[0][0].0, b'1');
    fixture::assert_success(&batches[1]);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
}

#[test]
fn should_prefer_input_errors_given_unresolved_trailing_slots_when_no_parameters_are_referenced() {
    // Arrange
    let steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "p",
        "SELECT 'bad'::INT",
        &[0, 0],
    ))];
    // Act
    let batches = run_script("parse-input-before-trailing-slot", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}

#[test]
fn should_reject_unresolved_trailing_slots_given_valid_analysis_when_slot_validation_runs() {
    // Arrange
    let steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "p",
        "SELECT 1::INT",
        &[23, 0],
    ))];
    // Act
    let batches = run_script("parse-unresolved-trailing-slot", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42P18"));
}

#[test]
fn should_prefer_input_errors_given_unresolved_referenced_slots_when_native_parse_visits_inputs() {
    // Arrange
    let steps = vec![Step::ParseFlush(wire::parse_frame(
        "p",
        "SELECT $1, 'bad'::INT",
    ))];
    // Act
    let batches = run_script("parse-input-before-unresolved-reference", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}
