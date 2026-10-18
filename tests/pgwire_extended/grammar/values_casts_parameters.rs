//! Native inferred and supplied typed VALUES parameters.
use crate::support_values_casts;

#[test]
fn should_preserve_date_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('2000-01-01'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-date_inferred",
        sql,
        0,
        &[50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
        &[0, 0, 34, 121],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1082);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('2000-01-01'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-date_supplied",
        sql,
        1082,
        &[50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
        &[0, 0, 34, 121],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1082);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('00:00:00'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-time_inferred",
        sql,
        0,
        &[49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50, 48, 48, 48, 48],
        &[0, 0, 0, 10, 139, 219, 240, 192],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1083);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('00:00:00'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-time_supplied",
        sql,
        1083,
        &[49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50, 48, 48, 48, 48],
        &[0, 0, 0, 10, 139, 219, 240, 192],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1083);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('2000-01-01 00:00:00'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-timestamp_inferred",
        sql,
        0,
        &[
            50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
            48, 48, 48, 48,
        ],
        &[0, 2, 181, 131, 65, 104, 80, 192],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1114);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1114, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 19, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49, 32, 48, 48, 58, 48,
                    48, 58, 48, 48,
                ],
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('2000-01-01 00:00:00'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-timestamp_supplied",
        sql,
        1114,
        &[
            50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
            48, 48, 48, 48,
        ],
        &[0, 2, 181, 131, 65, 104, 80, 192],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1114);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1114, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 19, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49, 32, 48, 48, 58, 48,
                    48, 58, 48, 48,
                ],
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('00000000-0000-0000-0000-000000000001'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-uuid_inferred",
        sql,
        0,
        &[
            65, 48, 69, 69, 66, 67, 57, 57, 45, 57, 67, 48, 66, 45, 52, 69, 70, 56, 45, 66, 66, 54,
            68, 45, 54, 66, 66, 57, 66, 68, 51, 56, 48, 65, 49, 49,
        ],
        &[
            160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189, 56, 10, 17,
        ],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 2950);
    support_values_casts::assert_result(
        &batches,
        ("column1", 2950, 16, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 36, 48, 48, 48, 48, 48, 48, 48, 48, 45, 48, 48, 48, 48, 45, 48,
                    48, 48, 48, 45, 48, 48, 48, 48, 45, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
                    49,
                ],
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('00000000-0000-0000-0000-000000000001'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-uuid_supplied",
        sql,
        2950,
        &[
            65, 48, 69, 69, 66, 67, 57, 57, 45, 57, 67, 48, 66, 45, 52, 69, 70, 56, 45, 66, 66, 54,
            68, 45, 54, 66, 66, 57, 66, 68, 51, 56, 48, 65, 49, 49,
        ],
        &[
            160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189, 56, 10, 17,
        ],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 2950);
    support_values_casts::assert_result(
        &batches,
        ("column1", 2950, 16, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 36, 48, 48, 48, 48, 48, 48, 48, 48, 45, 48, 48, 48, 48, 45, 48,
                    48, 48, 48, 45, 48, 48, 48, 48, 45, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
                    49,
                ],
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('\\x0102'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-bytea_inferred",
        sql,
        0,
        &[92, 120, 48, 48, 65, 98, 102, 102],
        &[0, 171, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
            ],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255], &[0, 1, 0, 0, 0, 2, 1, 2]],
        ],
    );
}

#[test]
fn should_preserve_bytea_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('\\x0102'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-bytea_supplied",
        sql,
        17,
        &[92, 120, 48, 48, 65, 98, 102, 102],
        &[0, 171, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
            ],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255], &[0, 1, 0, 0, 0, 2, 1, 2]],
        ],
    );
}

#[test]
fn should_preserve_char_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('b'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-char_inferred",
        sql,
        0,
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('b'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-char_supplied",
        sql,
        1042,
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_inferred_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('b'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-varchar_inferred",
        sql,
        0,
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_supplied_given_values_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "VALUES ($1),('b'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = support_values_casts::parameters(
        "valuescasts-param-varchar_supplied",
        sql,
        1043,
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
        &[108, 111, 110, 103, 240, 159, 153, 130, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}
