//! Native cast input diagnostics and recovery before result metadata.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_cast_input_0_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT int2($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-0", &[], cycles);
    // Assert
    assert_errors(&batches, "42725", "function int2(unknown) is not unique");
}

#[test]
fn should_reject_cast_input_1_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT int4($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-1", &[], cycles);
    // Assert
    assert_errors(&batches, "42725", "function int4(unknown) is not unique");
}

#[test]
fn should_reject_cast_input_2_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT int8($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-2", &[], cycles);
    // Assert
    assert_errors(&batches, "42725", "function int8(unknown) is not unique");
}

#[test]
fn should_reject_cast_input_3_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT float8($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-3", &[], cycles);
    // Assert
    assert_errors(&batches, "42725", "function float8(unknown) is not unique");
}

#[test]
fn should_reject_cast_input_4_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT bool($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-4", &[], cycles);
    // Assert
    assert_errors(&batches, "42725", "function bool(unknown) is not unique");
}

#[test]
fn should_reject_cast_input_5_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT bytea($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-5", &[], cycles);
    // Assert
    assert_errors(&batches, "42725", "function bytea(unknown) is not unique");
}

#[test]
fn should_reject_cast_input_6_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT pg_catalog.time($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-6", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42725",
        "function pg_catalog.time(unknown) is not unique",
    );
}

#[test]
fn should_reject_cast_input_7_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT uuid($1) AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-7", &[], cycles);
    // Assert
    assert_errors(&batches, "42883", "function uuid(unknown) does not exist");
}

#[test]
fn should_reject_cast_input_8_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT int2('32768') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-8", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22003",
        "value \"32768\" is out of range for type smallint",
    );
}

#[test]
fn should_reject_cast_input_9_given_invalid_type_function_input_when_native_diagnostic_is_observed()
{
    // Arrange
    let sql = "SELECT int4('bad') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-9", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type integer: \"bad\"",
    );
}

#[test]
fn should_reject_cast_input_10_given_invalid_type_function_input_when_native_diagnostic_is_observed(
) {
    // Arrange
    let sql = "SELECT uuid('bad') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-10", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type uuid: \"bad\"",
    );
}

#[test]
fn should_reject_cast_input_11_given_invalid_type_function_input_when_native_diagnostic_is_observed(
) {
    // Arrange
    let sql = "SELECT bool('bad') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cast-error-11", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type boolean: \"bad\"",
    );
}

fn assert_errors(batches: &[fixture::Frames], state: &str, message: &str) {
    for frames in batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some(state));
        assert!(!frames
            .iter()
            .any(|(tag, _)| matches!(tag, b'T' | b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        let body = &frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("error")
            .1;
        let actual = body
            .split(|byte| *byte == 0)
            .find_map(|part| part.strip_prefix(b"M"))
            .expect("message");
        assert_eq!(actual, message.as_bytes());
    }
}

#[test]
fn should_reject_uuid_form_0_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT uuid('a0eebc999c0b4ef8bb6d6bb9bd380a11-') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-uuid-error-0", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type uuid: \"a0eebc999c0b4ef8bb6d6bb9bd380a11-\"",
    );
}

#[test]
fn should_reject_uuid_form_1_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT uuid('{a0eebc999c0b4ef8bb6d6bb9bd380a11-}') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-uuid-error-1", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type uuid: \"{a0eebc999c0b4ef8bb6d6bb9bd380a11-}\"",
    );
}

#[test]
fn should_reject_uuid_form_2_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT uuid('-a0eebc999c0b4ef8bb6d6bb9bd380a11') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-uuid-error-2", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type uuid: \"-a0eebc999c0b4ef8bb6d6bb9bd380a11\"",
    );
}

#[test]
fn should_reject_uuid_form_3_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT uuid('a0ee--bc999c0b4ef8bb6d6bb9bd380a11') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-uuid-error-3", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type uuid: \"a0ee--bc999c0b4ef8bb6d6bb9bd380a11\"",
    );
}

#[test]
fn should_reject_float_form_0_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT float8('bad') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-float-error-0", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type double precision: \"bad\"",
    );
}

#[test]
fn should_reject_float_form_1_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT float8('1e400') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-float-error-1", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22003",
        "\"1e400\" is out of range for type double precision",
    );
}

#[test]
fn should_reject_float_form_2_given_invalid_text_when_native_error_is_reported() {
    // Arrange
    let sql = "SELECT float8('1e-400') AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("type-function-float-error-2", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22003",
        "\"1e-400\" is out of range for type double precision",
    );
}
