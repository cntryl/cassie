//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int2('32767') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 51, 50, 55, 54, 55]],
            &[&[0, 1, 0, 0, 0, 2, 127, 255]],
        ],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int2('-32768') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 45, 51, 50, 55, 54, 56]],
            &[&[0, 1, 0, 0, 0, 2, 128, 0]],
        ],
    );
}

#[test]
fn should_match_case_2_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int4('2147483647') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 49, 52, 55, 52, 56, 51, 54, 52, 55]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int4('-2147483648') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 11, 45, 50, 49, 52, 55, 52, 56, 51, 54, 52, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 128, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_4_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int8('9223372036854775807') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 19, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53,
                56, 48, 55,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 127, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_5_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int8('-9223372036854775808') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 20, 45, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55,
                53, 56, 48, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_6_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8('1.5') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 46, 53]],
            &[&[0, 1, 0, 0, 0, 8, 63, 248, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_7_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8('NaN') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-7", sql);
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
fn should_match_case_8_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8('Infinity') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 73, 110, 102, 105, 110, 105, 116, 121]],
            &[&[0, 1, 0, 0, 0, 8, 127, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_9_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8('-Infinity') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-9", sql);
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
fn should_match_case_10_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8('-0') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 48]],
            &[&[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT bool('yes') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_12_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT bool('off') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT text(7) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 55]], &[&[0, 1, 0, 0, 0, 1, 55]]],
    );
}

#[test]
fn should_match_case_14_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT text(TRUE) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 116, 114, 117, 101]],
            &[&[0, 1, 0, 0, 0, 4, 116, 114, 117, 101]],
        ],
    );
}

#[test]
fn should_match_case_15_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT text(FALSE) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 102, 97, 108, 115, 101]],
            &[&[0, 1, 0, 0, 0, 5, 102, 97, 108, 115, 101]],
        ],
    );
}

#[test]
fn should_match_case_16_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT text(NULL::INTEGER) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_17_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT date('2001-02-16') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1082, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 48, 48, 49, 45, 48, 50, 45, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 156]],
        ],
    );
}

#[test]
fn should_match_case_18_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.time('20:38:40') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1083, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 50, 48, 58, 51, 56, 58, 52, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 17, 77, 209, 52, 0]],
        ],
    );
}

#[test]
fn should_match_case_19_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.timestamp('2001-02-16 20:38:40') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1114, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 19, 50, 48, 48, 49, 45, 48, 50, 45, 49, 54, 32, 50, 48, 58, 51, 56,
                58, 52, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 32, 113, 84, 111, 180, 0]],
        ],
    );
}

#[test]
fn should_match_case_20_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT uuid('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}

#[test]
fn should_match_case_21_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT bytea('\\x0001ff') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 48, 49, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_match_case_22_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.varchar('hello') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 104, 101, 108, 108, 111]],
            &[&[0, 1, 0, 0, 0, 5, 104, 101, 108, 108, 111]],
        ],
    );
}

#[test]
fn should_match_case_23_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int4(NULL) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-23", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_24_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8(NULL) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-24", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_25_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT bool(NULL) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-25", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_26_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT date(NULL) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1082, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_27_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT text('café') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-27", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 99, 97, 102, 195, 169]],
            &[&[0, 1, 0, 0, 0, 5, 99, 97, 102, 195, 169]],
        ],
    );
}

#[test]
fn should_match_case_28_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.int4('7') AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-28", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_match_case_29_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.text(7) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-29", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 55]], &[&[0, 1, 0, 0, 0, 1, 55]]],
    );
}

#[test]
fn should_match_case_30_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int8(int4('7')) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-30", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_match_case_31_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT float8(int8('7')) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-31", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 8, 64, 28, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_32_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT int4('7'::TEXT) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-32", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_match_case_33_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT date(pg_catalog.timestamp('2001-02-16 20:38:40')) AS output";
    // Act
    let batches = planning::pruned("ten-batch-7-33", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1082, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 48, 48, 49, 45, 48, 50, 45, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 156]],
        ],
    );
}

#[test]
fn should_accept_uuid_form_0_given_native_text_when_type_function_is_executed() {
    // Arrange
    let sql = "SELECT uuid('a0eebc999c0b4ef8bb6d6bb9bd380a11') AS output";
    // Act
    let batches = planning::pruned("type-function-uuid-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}

#[test]
fn should_accept_uuid_form_1_given_native_text_when_type_function_is_executed() {
    // Arrange
    let sql = "SELECT uuid('{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}') AS output";
    // Act
    let batches = planning::pruned("type-function-uuid-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}

#[test]
fn should_accept_uuid_form_2_given_native_text_when_type_function_is_executed() {
    // Arrange
    let sql = "SELECT uuid('a0ee-bc99-9c0b-4ef8-bb6d-6bb9-bd38-0a11') AS output";
    // Act
    let batches = planning::pruned("type-function-uuid-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}

#[test]
fn should_accept_uuid_form_3_given_native_text_when_type_function_is_executed() {
    // Arrange
    let sql = "SELECT uuid('A0EEBC999C0B4EF8BB6D6BB9BD380A11') AS output";
    // Act
    let batches = planning::pruned("type-function-uuid-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}
