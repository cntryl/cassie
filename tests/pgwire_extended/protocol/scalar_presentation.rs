//! Native scalar output must use declared identities, not String shapes.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_scalar_presentation::{FLOATS, TIMESTAMPS};
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_float8_wire_identity_given_native_bit_witnesses_when_parameter_formats_vary() {
    // Arrange
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in FLOATS {
        let binary = case.bits.to_be_bytes();
        for input in [0, 1] {
            for output in [0, 1] {
                let body = if input == 0 {
                    case.text.as_bytes()
                } else {
                    &binary
                };
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS value",
                    Some((701, input, Some(body))),
                    output,
                    true,
                ));
                expected.push((case, output));
            }
        }
    }
    // Act
    let batches = fixture::run_cycles("native-float8-output-formats", &[], cycles);
    // Assert
    assert_eq!(batches.len(), expected.len());
    for (batch, (case, output)) in batches.iter().zip(expected) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (701, 8, -1), output, true);
        let binary = case.bits.to_be_bytes();
        let body = if output == 0 {
            case.text.as_bytes()
        } else {
            &binary
        };
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(body)],
            "bits {:016x}",
            case.bits
        );
    }
}

#[test]
fn should_format_float8_text_given_binary_inputs_when_sql_casts_keep_native_rounding() {
    // Arrange
    let cycles = FLOATS
        .iter()
        .map(|case| {
            fixture::execute_cycle(
                "SELECT $1::TEXT",
                Some((701, 1, Some(&case.bits.to_be_bytes()))),
                0,
                true,
            )
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("native-float8-binary-text-casts", &[], cycles);
    // Assert
    for (case, batch) in FLOATS.iter().zip(&batches) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (25, -1, -1), 0, true);
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(case.text.as_bytes())],
            "bits {:016x}",
            case.bits
        );
    }
}

#[test]
fn should_format_timestamp_text_given_native_precision_cases_when_parameter_formats_vary() {
    // Arrange
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in TIMESTAMPS {
        let binary = case.micros.to_be_bytes();
        for input in [0, 1] {
            for output in [0, 1] {
                let body = if input == 0 {
                    case.input.as_bytes()
                } else {
                    &binary
                };
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS value",
                    Some((1114, input, Some(body))),
                    output,
                    true,
                ));
                expected.push((case, output));
            }
        }
    }
    // Act
    let batches = fixture::run_cycles("native-timestamp-output-formats", &[], cycles);
    // Assert
    assert_eq!(batches.len(), expected.len());
    for (batch, (case, output)) in batches.iter().zip(expected) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (1114, 8, -1), output, true);
        let binary = case.micros.to_be_bytes();
        let body = if output == 0 {
            case.text.as_bytes()
        } else {
            &binary
        };
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(body)],
            "{}",
            case.input
        );
    }
}

#[test]
fn should_preserve_declared_source_types_given_identical_text_when_timestamp_parameters_are_cast() {
    // Arrange
    let input = "2024-02-29T12:34:56.120000Z";
    let batches = run_script(
        "native-cast-source-identity",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "typed",
                "SELECT $1::TEXT AS temporal, $2::TEXT AS genuine_text",
                &[1114, 25, 20],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("typed"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "out",
                    "typed",
                    &[],
                    &[Some(input.as_bytes()), Some(input.as_bytes()), Some(b"7")],
                    &[],
                ),
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let parameters = batches[1].iter().find(|frame| frame.0 == b't');
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::parse_parameter_description(&parameters.expect("typed trailing slots").1),
        vec![1114, 25, 20]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("2024-02-29 12:34:56.12".into()),
            Some(input.into())
        ]]
    );
}

#[test]
fn should_preserve_timestamp_source_types_given_case_and_coalesce_when_binary_parameters_are_cast()
{
    // Arrange
    let case = &TIMESTAMPS[3];
    let binary = case.micros.to_be_bytes();
    let cycle = fixture::execute_cycle(
        "SELECT (CASE WHEN TRUE THEN $1 ELSE NULL END)::TEXT, COALESCE($1, NULL)::TEXT",
        Some((1114, 1, Some(&binary))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("native-timestamp-conditional-source", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(case.text.into()); 2]]
    );
}

#[test]
fn should_recover_timestamp_text_casts_given_invalid_binary_values_when_a_named_statement_is_reused(
) {
    // Arrange
    let case = &TIMESTAMPS[1];
    let valid = case.micros.to_be_bytes();
    let invalid = (-9_223_372_036_854_775_807_i64).to_be_bytes();
    let batches = run_script(
        "native-timestamp-text-recovery",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "typed",
                "SELECT $1::TEXT",
                &[1114],
            )),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", "typed", &[1], &[Some(&invalid)], &[]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", "typed", &[1], &[Some(&valid)], &[]),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let state = wire::error_code(&batches[1]);
    // Assert
    assert_eq!(state.as_deref(), Some("22008"));
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some(case.text.into())]]
    );
}

#[test]
fn should_preserve_time_wire_identity_given_native_fraction_cases_when_parameters_execute() {
    // Arrange
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in crate::support_scalar_presentation::TIMES {
        let binary = case.micros.to_be_bytes();
        for input in [0, 1] {
            for output in [0, 1] {
                let body = if input == 0 {
                    case.input.as_bytes()
                } else {
                    &binary
                };
                cycles.push(fixture::execute_cycle(
                    "SELECT $1",
                    Some((1083, input, Some(body))),
                    output,
                    true,
                ));
                expected.push((case, output));
            }
        }
    }
    // Act
    let batches = fixture::run_cycles("native-time-output-formats", &[], cycles);
    // Assert
    for (batch, (case, output)) in batches.iter().zip(expected) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (1083, 8, -1), output, true);
        let binary = case.micros.to_be_bytes();
        let body = if output == 0 {
            case.text.as_bytes()
        } else {
            &binary
        };
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(body)],
            "{}",
            case.input
        );
    }
}

#[test]
fn should_preserve_time_source_identity_given_typed_parameters_when_text_casts_execute() {
    // Arrange
    let case = &crate::support_scalar_presentation::TIMES[2];
    let cycle = fixture::execute_cycle(
        "SELECT $1::TEXT",
        Some((1083, 1, Some(&case.micros.to_be_bytes()))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("native-time-text-cast-source", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(case.text.into())]]
    );
}

#[test]
fn should_recover_float8_text_parameters_given_native_input_errors_when_named_statements_are_rebound(
) {
    // Arrange
    let cases = [
        ("", "22P02"),
        ("wat", "22P02"),
        ("NaNstuff", "22P02"),
        ("1e9999", "22003"),
        ("1e-9999", "22003"),
        ("2e-324", "22003"),
        ("\u{a0}1.25", "22P02"),
    ];
    let mut steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "typed",
        "SELECT $1",
        &[701],
    ))];
    for (input, _) in cases {
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "typed", &[], &[Some(input.as_bytes())], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "typed", &[], &[Some(b"5e-324")], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
    }
    // Act
    let batches = run_script("native-float8-input-recovery", steps);
    // Assert
    for (index, (_, state)) in cases.iter().enumerate() {
        let rejected = &batches[index * 2 + 1];
        assert_eq!(wire::error_code(rejected).as_deref(), Some(*state));
        assert!(!rejected
            .iter()
            .any(|frame| matches!(frame.0, b'2' | b'D' | b'C')));
        fixture::assert_success(&batches[index * 2 + 2]);
        assert_eq!(
            wire::data_rows(&batches[index * 2 + 2]),
            vec![vec![Some("5e-324".into())]]
        );
    }
}

#[test]
fn should_preserve_unknown_literal_input_errors_given_pruned_consumers_when_statements_are_parsed()
{
    // Arrange
    let cases = [
        ("SELECT 1 FROM (VALUES ('bad'::INT)) v(n)", "22P02"),
        ("SELECT 1 FROM (VALUES ('40000'::SMALLINT)) v(n)", "22003"),
        (
            "SELECT CASE WHEN FALSE THEN '9223372036854775808'::BIGINT ELSE 0 END",
            "22003",
        ),
        ("SELECT FALSE AND ('1e9999'::FLOAT8=0)", "22003"),
    ];
    let steps = cases
        .iter()
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();
    // Act
    let batches = run_script("native-literal-input-phase", steps);
    // Assert
    for (batch, (_, state)) in batches.iter().zip(cases) {
        assert_eq!(wire::error_code(batch).as_deref(), Some(state));
        assert!(!batch.iter().any(|frame| frame.0 == b'D'));
    }
}

#[test]
fn should_preserve_float8_wire_identity_given_integer_storage_carriers_when_result_formats_vary() {
    // Arrange
    let binary = 9_007_199_254_740_992_f64.to_be_bytes();
    let setup = [
        ("CREATE TABLE integer_float(v FLOAT8)", vec![]),
        ("INSERT INTO integer_float VALUES(9007199254740993)", vec![]),
    ];
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle("SELECT v FROM integer_float", None, format, true))
        .collect();
    // Act
    let batches = fixture::run_cycles("native-float8-integer-wire-carrier", &setup, cycles);
    // Assert
    for (batch, format) in batches.iter().zip([0, 1]) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (701, 8, -1), format, true);
        let body = if format == 0 {
            b"9.007199254740992e+15".as_slice()
        } else {
            &binary
        };
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(body)]
        );
    }
}
