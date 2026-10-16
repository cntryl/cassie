//! Primitive wire results and errors for exact transient integer magnitudes.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_report_numeric_range_errors_given_live_casts_when_statements_execute() {
    // Arrange
    let sql = [
        "SELECT 9223372036854775808::BIGINT",
        "SELECT -9223372036854775808::BIGINT",
        "SELECT 18446744073709551615::INT",
        "SELECT 18446744073709551615::SMALLINT",
        "SELECT CASE WHEN TRUE THEN 9223372036854775808::BIGINT ELSE 0 END",
    ];
    let steps = sql
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .into();
    // Act
    let batches = run_script("exact-integer-live-errors", steps);
    // Assert
    for batch in batches {
        assert_eq!(wire::error_code(&batch).as_deref(), Some("22003"));
        assert!(!batch.iter().any(|frame| frame.0 == b'D'));
    }
}

#[test]
fn should_preserve_text_oid_given_exact_integer_casts_when_result_formats_vary() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle("SELECT 18446744073709551615::TEXT", None, format, true)
        })
        .into();
    // Act
    let batches = fixture::run_cycles("exact-integer-text-wire", &[], cycles);
    // Assert
    for (batch, format) in batches.iter().zip([0, 1]) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (25, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(b"18446744073709551615")]
        );
    }
}

#[test]
fn should_preserve_float8_oid_given_exact_integer_casts_when_result_formats_vary() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle("SELECT 18446744073709551615::FLOAT8", None, format, true)
        })
        .into();
    // Act
    let batches = fixture::run_cycles("exact-integer-float8-wire", &[], cycles);
    // Assert
    for (batch, format) in batches.iter().zip([0, 1]) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (701, 8, -1), format, true);
        let binary = 18_446_744_073_709_552_000_f64.to_be_bytes();
        let body = if format == 0 {
            b"1.8446744073709552e+19".as_slice()
        } else {
            &binary
        };
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(body)]
        );
    }
}

#[test]
fn should_recover_after_numeric_range_errors_given_sync_when_the_connection_is_reused() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 9223372036854775808::BIGINT",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 9223372036854775808::TEXT",
        )]),
    ];
    // Act
    let batches = run_script("exact-integer-range-recovery", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("9223372036854775808".into())]]
    );
}

#[test]
fn should_enforce_numeric_digit_limits_given_dead_integer_conversions_when_statements_are_parsed() {
    // Arrange
    let cases = [131_072, 131_073].map(|digits| {
        format!(
            "SELECT CASE WHEN FALSE THEN {}::BIGINT ELSE 0 END",
            "9".repeat(digits)
        )
    });
    let steps = cases
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();
    // Act
    let batches = run_script("exact-integer-native-digit-limit", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("0".into())]]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22003"));
    assert!(!batches[1].iter().any(|frame| frame.0 == b'D'));
}

#[test]
fn should_preserve_varchar_typmods_given_exact_numeric_casts_when_result_formats_vary() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle("SELECT 9223372036854775808::VARCHAR(5)", None, format, true)
        })
        .into();
    // Act
    let batches = fixture::run_cycles("exact-integer-varchar-wire", &[], cycles);
    // Assert
    for (batch, format) in batches.iter().zip([0, 1]) {
        fixture::assert_success(batch);
        fixture::assert_single_column_descriptors(batch, (1043, -1, 9), format, true);
        assert_eq!(
            fixture::data_row_payloads(batch),
            vec![fixture::one_field_row(b"92233")]
        );
    }
}

#[test]
fn should_preserve_supplied_parameter_slots_given_exact_numeric_casts_when_statements_are_described(
) {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT 9223372036854775808::TEXT",
            &[23, 20],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "out",
                "p",
                &[],
                &[Some(b"7"), Some(b"9007199254740993")],
                &[1],
            ),
            wire::describe_portal_frame("out"),
            wire::execute_frame("out"),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("exact-integer-trailing-slots", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    fixture::assert_success(&batches[1]);
    fixture::assert_success(&batches[2]);
    let parameters = batches[1]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![23, 20]
    );
    fixture::assert_single_column_descriptors(&batches[2], (25, -1, -1), 1, false);
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        vec![fixture::one_field_row(b"9223372036854775808")]
    );
}
