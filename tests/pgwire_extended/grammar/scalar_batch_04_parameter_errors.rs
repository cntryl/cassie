//! Integer absolute-value overflow preserves declared parameters and protocol recovery.
use crate::support_unary_math_parameter_recovery as recovery;

#[test]
fn should_recover_small_given_minimum_parameter_when_absolute_value_overflows() {
    // Arrange
    let case = recovery::Case {
        label: "unary-min-small",
        sql: "SELECT @ $1 AS output",
        oid: 21,
        bad_text: b"-32768",
        bad_binary: &[128, 0],
        good_binary: &[0, 1],
        expected: [&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 0, 0, 0, 2, 0, 1]],
    };
    // Act
    let batches = recovery::run(&case);
    // Assert
    recovery::assert_result(&case, &batches);
}

#[test]
fn should_recover_int_given_minimum_parameter_when_absolute_value_overflows() {
    // Arrange
    let case = recovery::Case {
        label: "unary-min-int",
        sql: "SELECT @ $1 AS output",
        oid: 23,
        bad_text: b"-2147483648",
        bad_binary: &[128, 0, 0, 0],
        good_binary: &[0, 0, 0, 1],
        expected: [&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
    };
    // Act
    let batches = recovery::run(&case);
    // Assert
    recovery::assert_result(&case, &batches);
}

#[test]
fn should_recover_big_given_minimum_parameter_when_absolute_value_overflows() {
    // Arrange
    let case = recovery::Case {
        label: "unary-min-big",
        sql: "SELECT @ $1 AS output",
        oid: 20,
        bad_text: b"-9223372036854775808",
        bad_binary: &[128, 0, 0, 0, 0, 0, 0, 0],
        good_binary: &[0, 0, 0, 0, 0, 0, 0, 1],
        expected: [
            &[0, 1, 0, 0, 0, 1, 49],
            &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
        ],
    };
    // Act
    let batches = recovery::run(&case);
    // Assert
    recovery::assert_result(&case, &batches);
}
