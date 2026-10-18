//! PostgreSQL 18.6 fixed BYTEA value and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_60_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha256('\\x00ff'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-60", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 66, 92, 120, 48, 54, 101, 98, 55, 100, 54, 97, 54, 57, 101, 101, 49,
                57, 101, 53, 102, 98, 100, 102, 55, 52, 57, 48, 49, 56, 100, 51, 100, 50, 97, 98,
                102, 97, 48, 52, 98, 99, 98, 100, 49, 51, 54, 53, 100, 98, 51, 49, 50, 101, 98, 56,
                54, 100, 99, 55, 49, 54, 57, 51, 56, 57, 98, 56,
            ]],
            &[&[
                0, 1, 0, 0, 0, 32, 6, 235, 125, 106, 105, 238, 25, 229, 251, 223, 116, 144, 24,
                211, 210, 171, 250, 4, 188, 189, 19, 101, 219, 49, 46, 184, 109, 199, 22, 147, 137,
                184,
            ]],
        ],
    );
}

#[test]
fn should_match_case_61_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha256(NULL::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-61", sql);
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
fn should_match_case_62_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha384('\\x'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-62", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 98, 92, 120, 51, 56, 98, 48, 54, 48, 97, 55, 53, 49, 97, 99, 57, 54,
                51, 56, 52, 99, 100, 57, 51, 50, 55, 101, 98, 49, 98, 49, 101, 51, 54, 97, 50, 49,
                102, 100, 98, 55, 49, 49, 49, 52, 98, 101, 48, 55, 52, 51, 52, 99, 48, 99, 99, 55,
                98, 102, 54, 51, 102, 54, 101, 49, 100, 97, 50, 55, 52, 101, 100, 101, 98, 102,
                101, 55, 54, 102, 54, 53, 102, 98, 100, 53, 49, 97, 100, 50, 102, 49, 52, 56, 57,
                56, 98, 57, 53, 98,
            ]],
            &[&[
                0, 1, 0, 0, 0, 48, 56, 176, 96, 167, 81, 172, 150, 56, 76, 217, 50, 126, 177, 177,
                227, 106, 33, 253, 183, 17, 20, 190, 7, 67, 76, 12, 199, 191, 99, 246, 225, 218,
                39, 78, 222, 191, 231, 111, 101, 251, 213, 26, 210, 241, 72, 152, 185, 91,
            ]],
        ],
    );
}

#[test]
fn should_match_case_63_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha384('\\x616263'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-63", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 98, 92, 120, 99, 98, 48, 48, 55, 53, 51, 102, 52, 53, 97, 51, 53,
                101, 56, 98, 98, 53, 97, 48, 51, 100, 54, 57, 57, 97, 99, 54, 53, 48, 48, 55, 50,
                55, 50, 99, 51, 50, 97, 98, 48, 101, 100, 101, 100, 49, 54, 51, 49, 97, 56, 98, 54,
                48, 53, 97, 52, 51, 102, 102, 53, 98, 101, 100, 56, 48, 56, 54, 48, 55, 50, 98, 97,
                49, 101, 55, 99, 99, 50, 51, 53, 56, 98, 97, 101, 99, 97, 49, 51, 52, 99, 56, 50,
                53, 97, 55,
            ]],
            &[&[
                0, 1, 0, 0, 0, 48, 203, 0, 117, 63, 69, 163, 94, 139, 181, 160, 61, 105, 154, 198,
                80, 7, 39, 44, 50, 171, 14, 222, 209, 99, 26, 139, 96, 90, 67, 255, 91, 237, 128,
                134, 7, 43, 161, 231, 204, 35, 88, 186, 236, 161, 52, 200, 37, 167,
            ]],
        ],
    );
}

#[test]
fn should_match_case_64_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha384('\\x00ff'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-64", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 98, 92, 120, 97, 102, 49, 102, 48, 98, 100, 56, 55, 54, 57, 101,
                100, 49, 51, 54, 53, 55, 50, 55, 100, 99, 53, 102, 56, 98, 97, 57, 100, 53, 101,
                54, 54, 101, 101, 54, 52, 98, 98, 97, 100, 99, 54, 49, 99, 102, 51, 56, 50, 57, 51,
                97, 98, 99, 56, 100, 55, 49, 100, 56, 53, 49, 102, 53, 98, 98, 100, 51, 97, 99, 57,
                98, 51, 97, 56, 97, 98, 56, 57, 57, 101, 54, 97, 48, 49, 101, 102, 100, 55, 101,
                56, 50, 101, 101, 48, 56,
            ]],
            &[&[
                0, 1, 0, 0, 0, 48, 175, 31, 11, 216, 118, 158, 209, 54, 87, 39, 220, 95, 139, 169,
                213, 230, 110, 230, 75, 186, 220, 97, 207, 56, 41, 58, 188, 141, 113, 216, 81, 245,
                187, 211, 172, 155, 58, 138, 184, 153, 230, 160, 30, 253, 126, 130, 238, 8,
            ]],
        ],
    );
}

#[test]
fn should_match_case_65_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha384(NULL::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-65", sql);
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
fn should_match_case_66_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha512('\\x'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-66", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 130, 92, 120, 99, 102, 56, 51, 101, 49, 51, 53, 55, 101, 101, 102,
                98, 56, 98, 100, 102, 49, 53, 52, 50, 56, 53, 48, 100, 54, 54, 100, 56, 48, 48, 55,
                100, 54, 50, 48, 101, 52, 48, 53, 48, 98, 53, 55, 49, 53, 100, 99, 56, 51, 102, 52,
                97, 57, 50, 49, 100, 51, 54, 99, 101, 57, 99, 101, 52, 55, 100, 48, 100, 49, 51,
                99, 53, 100, 56, 53, 102, 50, 98, 48, 102, 102, 56, 51, 49, 56, 100, 50, 56, 55,
                55, 101, 101, 99, 50, 102, 54, 51, 98, 57, 51, 49, 98, 100, 52, 55, 52, 49, 55, 97,
                56, 49, 97, 53, 51, 56, 51, 50, 55, 97, 102, 57, 50, 55, 100, 97, 51, 101,
            ]],
            &[&[
                0, 1, 0, 0, 0, 64, 207, 131, 225, 53, 126, 239, 184, 189, 241, 84, 40, 80, 214,
                109, 128, 7, 214, 32, 228, 5, 11, 87, 21, 220, 131, 244, 169, 33, 211, 108, 233,
                206, 71, 208, 209, 60, 93, 133, 242, 176, 255, 131, 24, 210, 135, 126, 236, 47, 99,
                185, 49, 189, 71, 65, 122, 129, 165, 56, 50, 122, 249, 39, 218, 62,
            ]],
        ],
    );
}

#[test]
fn should_match_case_67_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha512('\\x616263'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-67", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 130, 92, 120, 100, 100, 97, 102, 51, 53, 97, 49, 57, 51, 54, 49, 55,
                97, 98, 97, 99, 99, 52, 49, 55, 51, 52, 57, 97, 101, 50, 48, 52, 49, 51, 49, 49,
                50, 101, 54, 102, 97, 52, 101, 56, 57, 97, 57, 55, 101, 97, 50, 48, 97, 57, 101,
                101, 101, 101, 54, 52, 98, 53, 53, 100, 51, 57, 97, 50, 49, 57, 50, 57, 57, 50, 97,
                50, 55, 52, 102, 99, 49, 97, 56, 51, 54, 98, 97, 51, 99, 50, 51, 97, 51, 102, 101,
                101, 98, 98, 100, 52, 53, 52, 100, 52, 52, 50, 51, 54, 52, 51, 99, 101, 56, 48,
                101, 50, 97, 57, 97, 99, 57, 52, 102, 97, 53, 52, 99, 97, 52, 57, 102,
            ]],
            &[&[
                0, 1, 0, 0, 0, 64, 221, 175, 53, 161, 147, 97, 122, 186, 204, 65, 115, 73, 174, 32,
                65, 49, 18, 230, 250, 78, 137, 169, 126, 162, 10, 158, 238, 230, 75, 85, 211, 154,
                33, 146, 153, 42, 39, 79, 193, 168, 54, 186, 60, 35, 163, 254, 235, 189, 69, 77,
                68, 35, 100, 60, 232, 14, 42, 154, 201, 79, 165, 76, 164, 159,
            ]],
        ],
    );
}

#[test]
fn should_match_case_68_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha512('\\x00ff'::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-68", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 130, 92, 120, 102, 98, 57, 99, 48, 52, 50, 57, 52, 56, 55, 54, 53,
                53, 48, 56, 50, 102, 101, 49, 57, 55, 102, 54, 52, 101, 98, 56, 102, 48, 52, 53,
                97, 55, 48, 54, 97, 52, 97, 53, 100, 101, 56, 56, 49, 55, 52, 55, 56, 101, 101, 51,
                97, 98, 52, 55, 101, 50, 48, 56, 99, 50, 55, 102, 100, 56, 55, 53, 48, 50, 52, 53,
                56, 48, 50, 54, 54, 49, 51, 55, 57, 52, 101, 54, 57, 98, 48, 100, 49, 98, 51, 102,
                99, 54, 57, 49, 54, 52, 53, 54, 55, 53, 52, 98, 100, 48, 56, 48, 56, 56, 52, 50,
                50, 49, 52, 52, 53, 48, 51, 54, 56, 50, 100, 98, 53, 57, 56, 54,
            ]],
            &[&[
                0, 1, 0, 0, 0, 64, 251, 156, 4, 41, 72, 118, 85, 8, 47, 225, 151, 246, 78, 184,
                240, 69, 167, 6, 164, 165, 222, 136, 23, 71, 142, 227, 171, 71, 226, 8, 194, 127,
                216, 117, 2, 69, 128, 38, 97, 55, 148, 230, 155, 13, 27, 63, 198, 145, 100, 86,
                117, 75, 208, 128, 136, 66, 33, 68, 80, 54, 130, 219, 89, 134,
            ]],
        ],
    );
}

#[test]
fn should_match_case_69_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT sha512(NULL::BYTEA) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-69", sql);
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
fn should_match_case_70_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode(('\\xabababababababababababababababababababababababababababababababababababababababababababababababababababababababababab')::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-70", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 81, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10, 113, 119, 61, 61,
            ]],
            &[&[
                0, 1, 0, 0, 0, 81, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10, 113, 119, 61, 61,
            ]],
        ],
    );
}

#[test]
fn should_match_case_71_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('AA=A','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-71", sql);
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
fn should_match_case_72_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_byte('\\x00'::BYTEA,0,-1) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-72", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 102, 102]],
            &[&[0, 1, 0, 0, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_match_case_73_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_byte('\\x00'::BYTEA,0,256) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-73", sql);
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
fn should_match_case_74_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\xabababababababababababababababababababababababababababababababababababababababababababababababababababababababab'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-74", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 76, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 115, 61,
            ]],
            &[&[
                0, 1, 0, 0, 0, 76, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 115, 61,
            ]],
        ],
    );
}

#[test]
fn should_match_case_75_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\xababababababababababababababababababababababababababababababababababababababababababababababababababababababababab'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-75", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 77, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10,
            ]],
            &[&[
                0, 1, 0, 0, 0, 77, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10,
            ]],
        ],
    );
}

#[test]
fn should_match_case_76_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\xabababababababababababababababababababababababababababababababababababababababababababababababababababababababababab'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-76", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 81, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10, 113, 119, 61, 61,
            ]],
            &[&[
                0, 1, 0, 0, 0, 81, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10, 113, 119, 61, 61,
            ]],
        ],
    );
}

#[test]
fn should_match_case_77_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\xabababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababab'::BYTEA,'base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-77", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 154, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 10,
            ]],
            &[&[
                0, 1, 0, 0, 0, 154, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 10, 113, 54, 117, 114,
                113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113,
                54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54,
                117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117,
                114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114, 113, 54, 117, 114,
                113, 54, 117, 114, 10,
            ]],
        ],
    );
}

#[test]
fn should_match_case_78_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x01090a0d7f80'::BYTEA,'escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-78", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 9, 1, 9, 10, 13, 127, 92, 50, 48, 48]],
            &[&[0, 1, 0, 0, 0, 9, 1, 9, 10, 13, 127, 92, 50, 48, 48]],
        ],
    );
}

#[test]
fn should_match_case_79_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_bit('\\x01'::BYTEA,0::BIGINT) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-79", sql);
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
