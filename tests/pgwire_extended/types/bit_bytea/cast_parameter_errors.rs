//! Failed BYTEA parameters retain their statement and permit a valid rebind.
use crate::support_bytea_parameter_recovery::{self as fixture, Case};

#[test]
fn should_rebind_int2_given_an_overlength_bytea_parameter_when_the_statement_is_retained() {
    // Arrange
    let case = Case {
        label: "byteacasts-parameter-error-int2",
        sql: "SELECT $1::INT2 AS output",
        bad_binary: &[0, 0, 0],
        bad_text: &[92, 120, 48, 48, 48, 48, 48, 48],
        expected: [&[0, 1, 0, 0, 0, 1, 48], &[0, 1, 0, 0, 0, 2, 0, 0]],
        oid: 21,
    };
    // Act
    let batches = fixture::run(&case);
    // Assert
    fixture::assert_result(&case, &batches);
}

#[test]
fn should_rebind_int4_given_an_overlength_bytea_parameter_when_the_statement_is_retained() {
    // Arrange
    let case = Case {
        label: "byteacasts-parameter-error-int4",
        sql: "SELECT $1::INT4 AS output",
        bad_binary: &[0, 0, 0, 0, 0],
        bad_text: &[92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48],
        expected: [&[0, 1, 0, 0, 0, 1, 48], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        oid: 23,
    };
    // Act
    let batches = fixture::run(&case);
    // Assert
    fixture::assert_result(&case, &batches);
}

#[test]
fn should_rebind_int8_given_an_overlength_bytea_parameter_when_the_statement_is_retained() {
    // Arrange
    let case = Case {
        label: "byteacasts-parameter-error-int8",
        sql: "SELECT $1::INT8 AS output",
        bad_binary: &[0, 0, 0, 0, 0, 0, 0, 0, 0],
        bad_text: &[
            92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
        ],
        expected: [
            &[0, 1, 0, 0, 0, 1, 48],
            &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
        oid: 20,
    };
    // Act
    let batches = fixture::run(&case);
    // Assert
    fixture::assert_result(&case, &batches);
}
