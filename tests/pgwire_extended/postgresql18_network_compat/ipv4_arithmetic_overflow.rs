//! PostgreSQL 18 IPv4 arithmetic overflow behavior tracked by #816.

use super::*;

fn run_ipv4_boundary_error(sql: &str, script: &str) -> Vec<Vec<(u8, Vec<u8>)>> {
    run_script(
        script,
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 47 AS recovered")]),
        ],
    )
}

fn assert_ipv4_boundary_error_and_recovery(batches: &[Vec<(u8, Vec<u8>)>]) {
    assert_eq!(batches.len(), 2);
    let error = &batches[0];
    assert_eq!(wire::error_code(error).as_deref(), Some("22003"));
    let error_fields = error
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("overflow ErrorResponse");
    assert!(
        error_fields
            .iter()
            .any(|(kind, value)| *kind == 'M' && value == "result is out of range"),
        "PostgreSQL 18.6 overflow message: {error_fields:?}"
    );
    assert!(
        !error
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
        "overflow must not return a partial result"
    );
    assert_eq!(
        error.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..]),
        "error should end at ReadyForQuery"
    );

    let recovered = &batches[1];
    assert_eq!(wire::error_code(recovered), None);
    assert_eq!(
        wire::data_rows(recovered),
        vec![vec![Some("47".to_owned())]]
    );
}

#[test]
fn should_reject_ipv4_increment_past_maximum_with_sqlstate() {
    // Arrange
    let sql = "SELECT '255.255.255.255'::inet + 1::bigint";

    // Act
    let batches = run_ipv4_boundary_error(sql, "issue-816-ipv4-increment-overflow");

    // Assert
    assert_ipv4_boundary_error_and_recovery(&batches);
}

#[test]
fn should_reject_ipv4_decrement_below_minimum_with_sqlstate() {
    // Arrange
    let sql = "SELECT '0.0.0.0'::inet - 1::bigint";

    // Act
    let batches = run_ipv4_boundary_error(sql, "issue-816-ipv4-decrement-underflow");

    // Assert
    assert_ipv4_boundary_error_and_recovery(&batches);
}
