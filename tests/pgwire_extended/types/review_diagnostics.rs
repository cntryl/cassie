//! Native PostgreSQL error fields survive runtime evaluation and Sync recovery.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;
use std::collections::BTreeMap;

#[test]
fn should_emit_base64_hint_given_truncated_stored_text_when_execute_fails_and_sync_recovers() {
    // Arrange
    let setup = [
        ("CREATE TABLE review_base64 (payload TEXT)", vec![]),
        (
            "INSERT INTO review_base64 VALUES ($1)",
            vec![Value::String("AQ".into())],
        ),
    ];
    let cycles = vec![
        fixture::execute_cycle(
            "SELECT decode(payload,'base64') FROM review_base64",
            None,
            0,
            false,
        ),
        fixture::execute_cycle("SELECT 1", None, 0, false),
    ];
    // Act
    let batches = fixture::run_cycles("review-base64-hint", &setup, cycles);
    // Assert
    let errors = batches[0]
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .map(|(_, body)| {
            wire::parse_error_fields(body)
                .into_iter()
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].get(&'C').map(String::as_str), Some("22023"));
    assert_eq!(
        errors[0].get(&'M').map(String::as_str),
        Some("invalid base64 end sequence")
    );
    assert_eq!(
        errors[0].get(&'H').map(String::as_str),
        Some("Input data is missing padding, is truncated, or is otherwise corrupted.")
    );
    assert!(batches[0].iter().any(|(tag, _)| *tag == b'1'));
    assert!(batches[0].iter().any(|(tag, _)| *tag == b'2'));
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(batches[0].last(), Some(&(b'Z', vec![b'I'])));
    fixture::assert_success(&batches[1]);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("1".into())]]);
}

#[test]
fn should_emit_integer_input_detail_given_invalid_stored_text_when_execute_fails_and_sync_recovers()
{
    // Arrange
    for (target, input, code, message) in [
        (
            "smallint",
            "oops",
            "22P02",
            "invalid input syntax for type smallint: \"oops\"",
        ),
        (
            "integer",
            "oops",
            "22P02",
            "invalid input syntax for type integer: \"oops\"",
        ),
        (
            "bigint",
            "oops",
            "22P02",
            "invalid input syntax for type bigint: \"oops\"",
        ),
        (
            "smallint",
            "32768",
            "22003",
            "value \"32768\" is out of range for type smallint",
        ),
        (
            "integer",
            "2147483648",
            "22003",
            "value \"2147483648\" is out of range for type integer",
        ),
        (
            "bigint",
            "9223372036854775808",
            "22003",
            "value \"9223372036854775808\" is out of range for type bigint",
        ),
    ] {
        let setup = [
            ("CREATE TABLE review_integer (payload TEXT)", vec![]),
            (
                "INSERT INTO review_integer VALUES ($1)",
                vec![Value::String(input.into())],
            ),
        ];
        let sql = format!("SELECT payload::{target} FROM review_integer");
        let cycles = vec![
            fixture::execute_cycle(&sql, None, 0, false),
            fixture::execute_cycle("SELECT 1", None, 0, false),
        ];
        // Act
        let batches = fixture::run_cycles("review-integer-detail", &setup, cycles);
        // Assert
        let errors = batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| {
                wire::parse_error_fields(body)
                    .into_iter()
                    .collect::<BTreeMap<_, _>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{target} input {input}");
        assert_eq!(errors[0].get(&'C').map(String::as_str), Some(code));
        assert_eq!(errors[0].get(&'M').map(String::as_str), Some(message));
        assert!(batches[0].iter().any(|(tag, _)| *tag == b'1'));
        assert!(batches[0].iter().any(|(tag, _)| *tag == b'2'));
        assert!(!batches[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(batches[0].last(), Some(&(b'Z', vec![b'I'])));
        fixture::assert_success(&batches[1]);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("1".into())]]);
    }
}
