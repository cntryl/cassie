//! PostgreSQL 18.6 fixed BYTEA value and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_40_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_bit('\\x80'::BYTEA,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-40", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_41_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_bit('\\x80'::BYTEA,7) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-41", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_42_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_bit('\\x01'::BYTEA,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-42", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_43_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_bit('\\x0001'::BYTEA,8) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-43", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_44_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_bit(NULL::BYTEA,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-44", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_45_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_bit('\\x80'::BYTEA,0,1) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-45", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 56, 49]],
            &[&[0, 1, 0, 0, 0, 1, 129]],
        ],
    );
}

#[test]
fn should_match_case_46_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_bit('\\x00ff'::BYTEA,8,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-46", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 102, 101]],
            &[&[0, 1, 0, 0, 0, 2, 0, 254]],
        ],
    );
}

#[test]
fn should_match_case_47_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_bit('\\x00ff'::BYTEA,15,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-47", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 55, 102]],
            &[&[0, 1, 0, 0, 0, 2, 0, 127]],
        ],
    );
}

#[test]
fn should_match_case_48_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_bit(NULL::BYTEA,0,1) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-48", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_49_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT bit_count('\\x1234567890'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-49", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 53]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 15]],
        ],
    );
}

#[test]
fn should_match_case_50_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT bit_count('\\x'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-50", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_51_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT bit_count(NULL::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-51", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_52_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT pg_catalog.get_byte('\\x80'::BYTEA,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-52", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 50, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 128]],
        ],
    );
}

#[test]
fn should_match_case_53_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT pg_catalog.decode('00ff','hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-53", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 102, 102]],
            &[&[0, 1, 0, 0, 0, 2, 0, 255]],
        ],
    );
}

#[test]
fn should_match_case_54_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha224('\\x'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-54", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 58, 92, 120, 100, 49, 52, 97, 48, 50, 56, 99, 50, 97, 51, 97, 50,
                98, 99, 57, 52, 55, 54, 49, 48, 50, 98, 98, 50, 56, 56, 50, 51, 52, 99, 52, 49, 53,
                97, 50, 98, 48, 49, 102, 56, 50, 56, 101, 97, 54, 50, 97, 99, 53, 98, 51, 101, 52,
                50, 102,
            ]],
            &[&[
                0, 1, 0, 0, 0, 28, 209, 74, 2, 140, 42, 58, 43, 201, 71, 97, 2, 187, 40, 130, 52,
                196, 21, 162, 176, 31, 130, 142, 166, 42, 197, 179, 228, 47,
            ]],
        ],
    );
}

#[test]
fn should_match_case_55_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha224('\\x616263'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-55", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 58, 92, 120, 50, 51, 48, 57, 55, 100, 50, 50, 51, 52, 48, 53, 100,
                56, 50, 50, 56, 54, 52, 50, 97, 52, 55, 55, 98, 100, 97, 50, 53, 53, 98, 51, 50,
                97, 97, 100, 98, 99, 101, 52, 98, 100, 97, 48, 98, 51, 102, 55, 101, 51, 54, 99,
                57, 100, 97, 55,
            ]],
            &[&[
                0, 1, 0, 0, 0, 28, 35, 9, 125, 34, 52, 5, 216, 34, 134, 66, 164, 119, 189, 162, 85,
                179, 42, 173, 188, 228, 189, 160, 179, 247, 227, 108, 157, 167,
            ]],
        ],
    );
}

#[test]
fn should_match_case_56_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha224('\\x00ff'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-56", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 58, 92, 120, 102, 54, 101, 54, 52, 48, 49, 100, 100, 55, 100, 48,
                54, 48, 100, 50, 51, 50, 97, 48, 102, 102, 54, 99, 48, 97, 100, 57, 50, 56, 51, 53,
                53, 48, 57, 57, 48, 101, 48, 50, 49, 54, 101, 99, 98, 101, 57, 52, 57, 57, 101, 49,
                98, 50, 48, 98,
            ]],
            &[&[
                0, 1, 0, 0, 0, 28, 246, 230, 64, 29, 215, 208, 96, 210, 50, 160, 255, 108, 10, 217,
                40, 53, 80, 153, 14, 2, 22, 236, 190, 148, 153, 225, 178, 11,
            ]],
        ],
    );
}

#[test]
fn should_match_case_57_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha224(NULL::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-57", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_58_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha256('\\x'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-58", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 66, 92, 120, 101, 51, 98, 48, 99, 52, 52, 50, 57, 56, 102, 99, 49,
                99, 49, 52, 57, 97, 102, 98, 102, 52, 99, 56, 57, 57, 54, 102, 98, 57, 50, 52, 50,
                55, 97, 101, 52, 49, 101, 52, 54, 52, 57, 98, 57, 51, 52, 99, 97, 52, 57, 53, 57,
                57, 49, 98, 55, 56, 53, 50, 98, 56, 53, 53,
            ]],
            &[&[
                0, 1, 0, 0, 0, 32, 227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153,
                111, 185, 36, 39, 174, 65, 228, 100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184,
                85,
            ]],
        ],
    );
}

#[test]
fn should_match_case_59_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha256('\\x616263'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-59", sql);
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
