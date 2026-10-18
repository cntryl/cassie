//! Native constant-planning error states and Sync recovery.
use crate::support_bind_planning as planning;

#[test]
fn should_report_error_0_given_numeric_domain_failure_when_constant_is_bound() {
    // Arrange
    let cases = &[("SELECT 0::FLOAT8 ^ (-1)::FLOAT8", "2201F")];
    // Act
    let batches = planning::failures("math-error-0", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_report_error_1_given_numeric_domain_failure_when_constant_is_bound() {
    // Arrange
    let cases = &[("SELECT (-1)::FLOAT8 ^ 0.5::FLOAT8", "2201F")];
    // Act
    let batches = planning::failures("math-error-1", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_report_error_2_given_numeric_domain_failure_when_constant_is_bound() {
    // Arrange
    let cases = &[("SELECT 2::FLOAT8 ^ 1024", "22003")];
    // Act
    let batches = planning::failures("math-error-2", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_report_error_3_given_numeric_domain_failure_when_constant_is_bound() {
    // Arrange
    let cases = &[("SELECT 1 % 0", "22012")];
    // Act
    let batches = planning::failures("math-error-3", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}
