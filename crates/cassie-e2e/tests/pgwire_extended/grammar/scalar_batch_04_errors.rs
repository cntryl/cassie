//! Native unary math errors at Bind and protocol recovery.
use crate::support_bind_planning as planning;

#[test]
fn should_report_boundary_0_given_unary_math_when_bind_folds_the_operand() {
    // Arrange
    let cases = &[("SELECT |/(-1)::FLOAT8 AS output", "2201F")];
    // Act
    let batches = planning::failures("unary-boundary-0", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_report_boundary_1_given_unary_math_when_bind_folds_the_operand() {
    // Arrange
    let cases = &[("SELECT @ (-32768)::SMALLINT AS output", "22003")];
    // Act
    let batches = planning::failures("unary-boundary-1", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_report_boundary_2_given_unary_math_when_bind_folds_the_operand() {
    // Arrange
    let cases = &[("SELECT @ (-2147483648)::INT AS output", "22003")];
    // Act
    let batches = planning::failures("unary-boundary-2", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_report_boundary_3_given_unary_math_when_bind_folds_the_operand() {
    // Arrange
    let cases = &[("SELECT @ (-9223372036854775808)::BIGINT AS output", "22003")];
    // Act
    let batches = planning::failures("unary-boundary-3", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}
