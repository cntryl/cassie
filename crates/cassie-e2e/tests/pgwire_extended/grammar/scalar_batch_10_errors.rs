//! Native materialization barriers preserve error timing and recovery.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_report_materialized_constant_error_given_unused_target_when_binding() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT 1/0 AS bad,1 AS good) SELECT good FROM chosen";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cte-materialized-constant-error", &[], cycles);
    // Assert
    assert_errors(&batches, 0, 1);
}

#[test]
fn should_report_materialized_row_error_given_consumer_filter_when_executing() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT x,10/x AS quotient FROM (VALUES(0),(1)) AS input(x)) SELECT quotient FROM chosen WHERE x<>0";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cte-materialized-row-error", &[], cycles);
    // Assert
    assert_errors(&batches, 1, 2);
}

fn assert_errors(batches: &[fixture::Frames], binds: usize, descriptions: usize) {
    for frames in batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("22012"));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'2').count(), binds);
        assert_eq!(
            frames.iter().filter(|(tag, _)| *tag == b'T').count(),
            descriptions
        );
        assert!(!frames.iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_ambiguous_cte_output_given_mode_0_when_binding() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT 1 AS value,2 AS value) SELECT value FROM chosen";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cte-duplicate-0", &[], cycles);
    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42702"));
        assert!(!frames.iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_ambiguous_cte_output_given_mode_1_when_binding() {
    // Arrange
    let sql =
        "WITH chosen AS NOT MATERIALIZED (SELECT 1 AS value,2 AS value) SELECT value FROM chosen";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("cte-duplicate-1", &[], cycles);
    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42702"));
        assert!(!frames.iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}
