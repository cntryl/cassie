//! Native FLOAT8 power special values and domain boundaries.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_special_0_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 'NaN'::FLOAT8 ^ 0 AS output";
    // Act
    let batches = planning::pruned("power-special-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 63, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_1_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 0::FLOAT8 ^ 'NaN'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("power-special-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 78, 97, 78]],
            &[&[0, 1, 0, 0, 0, 8, 127, 248, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_2_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 1::FLOAT8 ^ 'NaN'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("power-special-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 63, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_3_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 'NaN'::FLOAT8 ^ 2 AS output";
    // Act
    let batches = planning::pruned("power-special-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 78, 97, 78]],
            &[&[0, 1, 0, 0, 0, 8, 127, 248, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_4_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 'Infinity'::FLOAT8 ^ 0 AS output";
    // Act
    let batches = planning::pruned("power-special-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 63, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_5_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 'Infinity'::FLOAT8 ^ (-1) AS output";
    // Act
    let batches = planning::pruned("power-special-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_6_given_float_power_when_native_result_is_observed() {
    // Arrange
    let cases = &[(
        "SELECT '-Infinity'::FLOAT8 ^ 0.5::FLOAT8 AS output",
        "2201F",
    )];
    // Act
    let batches = planning::failures("power-special-6", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}

#[test]
fn should_preserve_special_7_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT '-Infinity'::FLOAT8 ^ 3 AS output";
    // Act
    let batches = planning::pruned("power-special-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 9, 45, 73, 110, 102, 105, 110, 105, 116, 121]],
            &[&[0, 1, 0, 0, 0, 8, 255, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_8_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT (-1)::FLOAT8 ^ 'Infinity'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("power-special-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 63, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_9_given_float_power_when_native_result_is_observed() {
    // Arrange
    let sql = "SELECT 0::FLOAT8 ^ 'Infinity'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("power-special-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_special_10_given_float_power_when_native_result_is_observed() {
    // Arrange
    let cases = &[("SELECT 0::FLOAT8 ^ '-Infinity'::FLOAT8 AS output", "2201F")];
    // Act
    let batches = planning::failures("power-special-10", cases);
    // Assert
    planning::assert_failures(&batches, cases);
}
