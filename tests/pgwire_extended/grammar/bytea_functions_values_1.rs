//! PostgreSQL 18.6 fixed BYTEA value and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_20_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('DEad00FF','hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 100, 101, 97, 100, 48, 48, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 222, 173, 0, 255]],
        ],
    );
}

#[test]
fn should_match_case_21_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('DE AD BE EF','hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 100, 101, 97, 100, 98, 101, 101, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 222, 173, 190, 239]],
        ],
    );
}

#[test]
fn should_match_case_22_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('MTIzAAE=','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 92, 120, 51, 49, 51, 50, 51, 51, 48, 48, 48, 49,
            ]],
            &[&[0, 1, 0, 0, 0, 5, 49, 50, 51, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_23_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('MTIz AA E=\n','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-23", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 92, 120, 51, 49, 51, 50, 51, 51, 48, 48, 48, 49,
            ]],
            &[&[0, 1, 0, 0, 0, 5, 49, 50, 51, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_24_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('A\\000\\\\\\377','escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-24", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 52, 49, 48, 48, 53, 99, 102, 102]],
            &[&[0, 1, 0, 0, 0, 4, 65, 0, 92, 255]],
        ],
    );
}

#[test]
fn should_match_case_25_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('','hex') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-25", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_26_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_27_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('','escape') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-27", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_match_case_28_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('YR==','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-28", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 54, 49]],
            &[&[0, 1, 0, 0, 0, 1, 97]],
        ],
    );
}

#[test]
fn should_match_case_29_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode(NULL::BYTEA,'bad') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-29", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_30_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\x00'::BYTEA,NULL::TEXT) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-30", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_31_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode(NULL::TEXT,'bad') AS v";
    // Act
    let batches = planning::pruned("bytea-functions-31", sql);
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
fn should_match_case_32_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT decode('bad',NULL::TEXT) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-32", sql);
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
fn should_match_case_33_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_byte('\\x80ff'::BYTEA,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-33", sql);
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
fn should_match_case_34_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_byte('\\x80ff'::BYTEA,1) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-34", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 255]],
        ],
    );
}

#[test]
fn should_match_case_35_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_byte(NULL::BYTEA,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-35", sql);
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
fn should_match_case_36_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT get_byte('\\x80'::BYTEA,NULL::INT) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-36", sql);
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
fn should_match_case_37_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_byte('\\x80ff'::BYTEA,0,1) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-37", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 102, 102]],
            &[&[0, 1, 0, 0, 0, 2, 1, 255]],
        ],
    );
}

#[test]
fn should_match_case_38_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_byte('\\x80ff'::BYTEA,1,255) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-38", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 56, 48, 102, 102]],
            &[&[0, 1, 0, 0, 0, 2, 128, 255]],
        ],
    );
}

#[test]
fn should_match_case_39_given_fixed_bytea_function_when_native_result_is_requested() {
    // Arrange
    let sql = "SELECT set_byte(NULL::BYTEA,0,0) AS v";
    // Act
    let batches = planning::pruned("bytea-functions-39", sql);
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
