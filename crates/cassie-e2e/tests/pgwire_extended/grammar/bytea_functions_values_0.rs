//! PostgreSQL 18.6 fixed BYTEA value and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x'::BYTEA,'hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 0]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_1_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x00'::BYTEA,'hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 48, 48]],
            &[&[0, 1, 0, 0, 0, 2, 48, 48]],
        ],
    );
}

#[test]
fn should_match_case_2_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x41005cff'::BYTEA,'hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 52, 49, 48, 48, 53, 99, 102, 102]],
            &[&[0, 1, 0, 0, 0, 8, 52, 49, 48, 48, 53, 99, 102, 102]],
        ],
    );
}

#[test]
fn should_match_case_3_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x80ff010203'::BYTEA,'hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 56, 48, 102, 102, 48, 49, 48, 50, 48, 51]],
            &[&[0, 1, 0, 0, 0, 10, 56, 48, 102, 102, 48, 49, 48, 50, 48, 51]],
        ],
    );
}

#[test]
fn should_match_case_4_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 0]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_5_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x00'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 65, 65, 61, 61]],
            &[&[0, 1, 0, 0, 0, 4, 65, 65, 61, 61]],
        ],
    );
}

#[test]
fn should_match_case_6_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x41005cff'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 81, 81, 66, 99, 47, 119, 61, 61]],
            &[&[0, 1, 0, 0, 0, 8, 81, 81, 66, 99, 47, 119, 61, 61]],
        ],
    );
}

#[test]
fn should_match_case_7_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x80ff010203'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 103, 80, 56, 66, 65, 103, 77, 61]],
            &[&[0, 1, 0, 0, 0, 8, 103, 80, 56, 66, 65, 103, 77, 61]],
        ],
    );
}

#[test]
fn should_match_case_8_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x'::BYTEA,'escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 0]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_9_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x00'::BYTEA,'escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 92, 48, 48, 48]],
        ],
    );
}

#[test]
fn should_match_case_10_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x41005cff'::BYTEA,'escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 11, 65, 92, 48, 48, 48, 92, 92, 92, 51, 55, 55,
            ]],
            &[&[
                0, 1, 0, 0, 0, 11, 65, 92, 48, 48, 48, 92, 92, 92, 51, 55, 55,
            ]],
        ],
    );
}

#[test]
fn should_match_case_11_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x80ff010203'::BYTEA,'escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 11, 92, 50, 48, 48, 92, 51, 55, 55, 1, 2, 3]],
            &[&[0, 1, 0, 0, 0, 11, 92, 50, 48, 48, 92, 51, 55, 55, 1, 2, 3]],
        ],
    );
}

#[test]
fn should_match_case_12_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x'::BYTEA,'HEX') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 0]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_13_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x00'::BYTEA,'HEX') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 48, 48]],
            &[&[0, 1, 0, 0, 0, 2, 48, 48]],
        ],
    );
}

#[test]
fn should_match_case_14_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x41005cff'::BYTEA,'HEX') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 52, 49, 48, 48, 53, 99, 102, 102]],
            &[&[0, 1, 0, 0, 0, 8, 52, 49, 48, 48, 53, 99, 102, 102]],
        ],
    );
}

#[test]
fn should_match_case_15_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x80ff010203'::BYTEA,'HEX') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 56, 48, 102, 102, 48, 49, 48, 50, 48, 51]],
            &[&[0, 1, 0, 0, 0, 10, 56, 48, 102, 102, 48, 49, 48, 50, 48, 51]],
        ],
    );
}

#[test]
fn should_match_case_16_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x'::BYTEA,'BaSe64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 0]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_17_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x00'::BYTEA,'BaSe64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 65, 65, 61, 61]],
            &[&[0, 1, 0, 0, 0, 4, 65, 65, 61, 61]],
        ],
    );
}

#[test]
fn should_match_case_18_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x41005cff'::BYTEA,'BaSe64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 81, 81, 66, 99, 47, 119, 61, 61]],
            &[&[0, 1, 0, 0, 0, 8, 81, 81, 66, 99, 47, 119, 61, 61]],
        ],
    );
}

#[test]
fn should_match_case_19_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x80ff010203'::BYTEA,'BaSe64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 103, 80, 56, 66, 65, 103, 77, 61]],
            &[&[0, 1, 0, 0, 0, 8, 103, 80, 56, 66, 65, 103, 77, 61]],
        ],
    );
}
