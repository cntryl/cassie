//! Input error states, phase boundaries and recovery on real connections.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_primitive_literal_inputs::INVALID;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_report_quoted_input_errors_given_dead_case_arms_when_simple_queries_execute() {
    // Arrange
    let steps = INVALID
        .iter()
        .map(|case| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "SELECT CASE WHEN FALSE THEN '{}'::{} ELSE NULL::{} END",
                case.input, case.sql_type, case.sql_type
            ))])
        })
        .collect();
    // Act
    let batches = run_script("primitive-input-case-states", steps);
    // Assert
    for (batch, case) in batches.iter().zip(INVALID) {
        assert_eq!(
            wire::error_code(batch).as_deref(),
            Some(case.state),
            "{} {:?}",
            case.sql_type,
            case.input
        );
        assert!(!batch.iter().any(|f| f.0 == b'D'));
    }
}

#[test]
fn should_report_quoted_input_errors_given_unused_values_fields_when_simple_queries_execute() {
    // Arrange
    let steps = INVALID
        .iter()
        .map(|case| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "SELECT 1 FROM (VALUES ('{}'::{})) v(n)",
                case.input, case.sql_type
            ))])
        })
        .collect();
    // Act
    let batches = run_script("primitive-input-values-states", steps);
    // Assert
    for (batch, case) in batches.iter().zip(INVALID) {
        assert_eq!(
            wire::error_code(batch).as_deref(),
            Some(case.state),
            "{} {:?}",
            case.sql_type,
            case.input
        );
        assert!(!batch.iter().any(|f| f.0 == b'D'));
    }
}

#[test]
fn should_preserve_name_error_order_given_invalid_quoted_inputs_when_projection_items_are_bound() {
    // Arrange
    let cases = [
        ("SELECT missing, 'bad'::INT", "42703"),
        ("SELECT 'bad'::INT, missing", "22P02"),
        ("SELECT 'bad'::INT FROM missing_relation", "42P01"),
    ];
    let steps = cases
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .into();
    // Act
    let batches = run_script("primitive-input-reference-order", steps);
    // Assert
    for (batch, (_, state)) in batches.iter().zip(cases) {
        assert_eq!(wire::error_code(batch).as_deref(), Some(state));
    }
}

#[test]
fn should_report_quoted_input_errors_given_deciding_boolean_operands_when_inputs_are_bound() {
    // Arrange
    let cases = [
        ("SELECT FALSE AND ('1e9999'::FLOAT8=0)", "22003"),
        ("SELECT TRUE OR 'bad'::BOOLEAN", "22P02"),
        ("SELECT FALSE AND ('40000'::SMALLINT=0)", "22003"),
    ];
    let steps = cases
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .into();
    // Act
    let batches = run_script("primitive-input-boolean-pruning", steps);
    // Assert
    for (batch, (_, state)) in batches.iter().zip(cases) {
        assert_eq!(wire::error_code(batch).as_deref(), Some(state));
    }
}

#[test]
fn should_reject_boolean_text_input_given_invalid_spellings_when_named_parameters_are_rebound() {
    // Arrange
    let cases = ["o", "", "truth", "2", "\u{a0}true", "false\u{2003}"];
    let mut steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "p",
        "SELECT $1",
        &[16],
    ))];
    for input in cases {
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "p", &[], &[Some(input.as_bytes())], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "p", &[], &[Some(b"t")], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
    }
    // Act
    let batches = run_script("primitive-boolean-bind-recovery", steps);
    // Assert
    for i in 0..cases.len() {
        let failed = &batches[i * 2 + 1];
        assert_eq!(wire::error_code(failed).as_deref(), Some("22P02"));
        assert!(!failed.iter().any(|f| matches!(f.0, b'2' | b'D' | b'C')));
        fixture::assert_success(&batches[i * 2 + 2]);
        assert_eq!(
            wire::data_rows(&batches[i * 2 + 2]),
            vec![vec![Some("t".into())]]
        );
    }
}

#[test]
fn should_reject_quoted_input_errors_given_parse_flush_when_metadata_has_not_been_published() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT CASE WHEN FALSE THEN 'bad'::INT ELSE 0 END",
            &[23, 20],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT 7::INT",
            &[23, 20],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "p", &[], &[Some(b"1"), Some(b"2")], &[1]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("primitive-input-parse-recovery", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert!(!batches[0].iter().any(|f| matches!(f.0, b'1' | b't' | b'T')));
    assert_eq!(
        batches[1].iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![b'Z']
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    fixture::assert_success(&batches[3]);
    assert_eq!(
        fixture::data_row_payloads(&batches[3]),
        vec![fixture::one_field_row(&7_i32.to_be_bytes())]
    );
}

#[test]
fn should_report_quoted_input_errors_given_exists_targets_when_output_expressions_are_pruned() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT EXISTS(SELECT 'bad'::INT)",
    )])];
    // Act
    let batches = run_script("primitive-input-exists-state", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}

#[test]
fn should_report_quoted_input_errors_given_unused_ctes_when_relations_are_bound() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "WITH unused AS (SELECT 'bad'::INT) SELECT 1",
    )])];
    // Act
    let batches = run_script("primitive-input-cte-state", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}

#[test]
fn should_report_typed_text_input_errors_given_false_filters_when_immutable_conversions_are_folded()
{
    // Arrange
    let steps = INVALID
        .iter()
        .map(|case| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "SELECT CAST('{}' AS TEXT)::{} WHERE FALSE",
                case.input, case.sql_type
            ))])
        })
        .collect();
    // Act
    let batches = run_script("primitive-input-typed-text-fold-states", steps);
    // Assert
    for (batch, case) in batches.iter().zip(INVALID) {
        assert_eq!(
            wire::error_code(batch).as_deref(),
            Some(case.state),
            "{} {:?}",
            case.sql_type,
            case.input
        );
    }
}

#[test]
fn should_preserve_boolean_oid_given_valid_text_inputs_when_result_formats_vary() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle("SELECT $1", Some((16, 0, Some(b"of"))), format, true))
        .into();
    // Act
    let batches = fixture::run_cycles("primitive-input-boolean-wire", &[], cycles);
    // Assert
    for (batch, format) in batches.iter().zip([0, 1]) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (16, 1, -1), format, true);
        let body = if format == 0 { b"f".as_slice() } else { &[0] };
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(body)]
        );
    }
}

#[test]
fn should_recover_given_large_invalid_boolean_literals_when_input_validation_fails() {
    // Arrange
    let sql = format!("SELECT '{}'::BOOLEAN", "x".repeat(128 * 1024));
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(&sql)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
    ];
    // Act
    let batches = run_script("primitive-input-bounded-errors", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert!(!batches[0].iter().any(|frame| frame.0 == b'D'));
    fixture::assert_success(&batches[1]);
}

#[test]
fn should_match_native_boolean_inputs_given_prefixes_and_control_characters_when_result_formats_vary(
) {
    // Arrange
    let cases = crate::support_primitive_literal_inputs::BOOLEANS;
    let cycles = cases
        .iter()
        .flat_map(|(input, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    "SELECT $1",
                    Some((16, 0, Some(input.as_bytes()))),
                    format,
                    true,
                )
            })
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("primitive-native-boolean-format-matrix", &[], cycles);
    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for (case, outputs) in cases.iter().zip(batches.as_chunks::<2>().0) {
        for (batch, format) in outputs.iter().zip([0, 1]) {
            if let Some(value) = case.1 {
                fixture::assert_success(batch);
                fixture::assert_single_column_descriptors(batch, (16, 1, -1), format, true);
                let binary = [u8::from(value)];
                let body = if format == 0 {
                    if value {
                        b"t".as_slice()
                    } else {
                        b"f".as_slice()
                    }
                } else {
                    &binary
                };
                assert_eq!(
                    fixture::data_row_payloads(batch),
                    vec![fixture::one_field_row(body)],
                    "{:?}",
                    case.0
                );
            } else {
                assert_eq!(
                    wire::error_code(batch).as_deref(),
                    Some("22P02"),
                    "{:?}",
                    case.0
                );
                assert!(!batch
                    .iter()
                    .any(|frame| matches!(frame.0, b'2' | b'D' | b'C')));
            }
        }
    }
}
