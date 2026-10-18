//! PostgreSQL 18.6 fixed BYTEA value and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_80_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_bit('\\x01'::BYTEA,0::BIGINT,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-80", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_match_case_81_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha256('abc') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-81", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 66, 92, 120, 98, 97, 55, 56, 49, 54, 98, 102, 56, 102, 48, 49, 99,
                102, 101, 97, 52, 49, 52, 49, 52, 48, 100, 101, 53, 100, 97, 101, 50, 50, 50, 51,
                98, 48, 48, 51, 54, 49, 97, 51, 57, 54, 49, 55, 55, 97, 57, 99, 98, 52, 49, 48,
                102, 102, 54, 49, 102, 50, 48, 48, 49, 53, 97, 100,
            ]],
            &[&[
                0, 1, 0, 0, 0, 32, 186, 120, 22, 191, 143, 1, 207, 234, 65, 65, 64, 222, 93, 174,
                34, 35, 176, 3, 97, 163, 150, 23, 122, 156, 180, 16, 255, 97, 242, 0, 21, 173,
            ]],
        ],
    );
}

#[test]
fn should_match_case_82_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('abc','hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-82", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 54, 49, 54, 50, 54, 51]],
            &[&[0, 1, 0, 0, 0, 6, 54, 49, 54, 50, 54, 51]],
        ],
    );
}

#[test]
fn should_match_case_83_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AA=A','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-83", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_match_case_84_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AB=C','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-84", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_match_case_85_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AA=Z','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-85", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_match_case_86_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AA==AAAA','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-86", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 2, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_87_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AA==AA==','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-87", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 2, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_88_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('YQ==YQ==','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-88", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 54, 49, 54, 49]],
            &[&[0, 1, 0, 0, 0, 2, 97, 97]],
        ],
    );
}

#[test]
fn should_match_case_89_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AAA=','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-89", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 2, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_90_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('A A = A','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-90", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}
