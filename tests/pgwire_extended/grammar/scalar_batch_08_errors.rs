//! Native LIKE ESCAPE diagnostics and recovery.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_like_input_0_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x' ESCAPE '!!' AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-0", &[], cycles);
    // Assert
    assert_errors(&batches, "22025", "invalid escape string");
}

#[test]
fn should_reject_like_input_1_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT NULL::TEXT LIKE 'x' ESCAPE '!!' AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-1", &[], cycles);
    // Assert
    assert_errors(&batches, "22025", "invalid escape string");
}

#[test]
fn should_reject_like_input_2_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'xx' LIKE 'x!' ESCAPE '!' AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-2", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22025",
        "LIKE pattern must not end with escape character",
    );
}

#[test]
fn should_reject_like_input_3_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'x' LIKE '%' ESCAPE '%' AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-3", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22025",
        "LIKE pattern must not end with escape character",
    );
}

#[test]
fn should_reject_like_input_4_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT v LIKE p ESCAPE '!!' AS output FROM (VALUES(NULL::TEXT,'x')) AS t(v,p)";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-4", &[], cycles);
    // Assert
    assert_errors(&batches, "22025", "invalid escape string");
}

#[test]
fn should_reject_like_input_5_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 1 ESCAPE '!' AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-5", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42883",
        "function pg_catalog.like_escape(integer, unknown) does not exist",
    );
}

#[test]
fn should_reject_like_input_6_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x' ESCAPE 1 AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-6", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42883",
        "function pg_catalog.like_escape(unknown, integer) does not exist",
    );
}

#[test]
fn should_reject_like_input_7_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 1 LIKE 'x' ESCAPE '!' AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-7", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42883",
        "operator does not exist: integer ~~ text",
    );
}

#[test]
fn should_reject_like_input_8_given_invalid_operands_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'x' LIKE $1 ESCAPE 1 AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((0, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-error-8", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42883",
        "function pg_catalog.like_escape(unknown, integer) does not exist",
    );
}
fn assert_errors(batches: &[fixture::Frames], state: &str, message: &str) {
    for frames in batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some(state));
        // Native Describe(statement) returns one descriptor before Bind planning
        // reports 22025. Parse-time type errors return none.
        assert_eq!(
            frames.iter().filter(|(tag, _)| *tag == b'T').count(),
            usize::from(state == "22025")
        );
        assert!(!frames
            .iter()
            .any(|(tag, _)| matches!(tag, b'2' | b'D' | b'C')));
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
fn should_reject_like_chain_0_given_unparenthesized_predicates_when_parsing() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x' LIKE 't'";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-chain-0", &[], cycles);
    // Assert
    assert_errors(&batches, "42601", "syntax error at or near \"LIKE\"");
}

#[test]
fn should_reject_like_chain_1_given_unparenthesized_predicates_when_parsing() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x' ESCAPE '!' LIKE 't'";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-chain-1", &[], cycles);
    // Assert
    assert_errors(&batches, "42601", "syntax error at or near \"LIKE\"");
}

#[test]
fn should_reject_like_chain_2_given_unparenthesized_predicates_when_parsing() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!' LIKE TRUE";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-chain-2", &[], cycles);
    // Assert
    assert_errors(&batches, "42601", "syntax error at or near \"LIKE\"");
}

#[test]
fn should_reject_like_chain_3_given_unparenthesized_predicates_when_parsing() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x' NOT LIKE 't'";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-chain-3", &[], cycles);
    // Assert
    assert_errors(&batches, "42601", "syntax error at or near \"NOT\"");
}

#[test]
fn should_reject_like_chain_4_given_unparenthesized_predicates_when_parsing() {
    // Arrange
    let sql = "SELECT 'x' NOT LIKE 'x' LIKE 't'";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("like-chain-4", &[], cycles);
    // Assert
    assert_errors(&batches, "42601", "syntax error at or near \"LIKE\"");
}
