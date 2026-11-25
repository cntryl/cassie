//! Native explicit VARCHAR truncation payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_truncate_ascii_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('abcdef')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-ascii_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
        ],
    );
}

#[test]
fn should_truncate_exact_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('abc')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-exact_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
        ],
    );
}

#[test]
fn should_truncate_short_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('a')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-short_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [&[&[0, 1, 0, 0, 0, 1, 97]], &[&[0, 1, 0, 0, 0, 1, 97]]],
    );
}

#[test]
fn should_truncate_empty_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-empty_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [&[&[0, 1, 0, 0, 0, 0]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_truncate_spaces_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('a    ')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-spaces_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32]],
        ],
    );
}

#[test]
fn should_truncate_leading_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('   abc')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-leading_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 32, 32, 32]],
            &[&[0, 1, 0, 0, 0, 3, 32, 32, 32]],
        ],
    );
}

#[test]
fn should_truncate_unicode_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('a🙂文z')::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-unicode_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
        ],
    );
}

#[test]
fn should_truncate_emoji_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('🙂🙃🙂🙃')::VARCHAR(2))";
    // Act
    let batches = planning::pruned("varchar-emoji_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 6),
        [
            &[&[0, 1, 0, 0, 0, 8, 240, 159, 153, 130, 240, 159, 153, 131]],
            &[&[0, 1, 0, 0, 0, 8, 240, 159, 153, 130, 240, 159, 153, 131]],
        ],
    );
}

#[test]
fn should_truncate_combining_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('e\u{301}xy')::VARCHAR(2))";
    // Act
    let batches = planning::pruned("varchar-combining_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 6),
        [
            &[&[0, 1, 0, 0, 0, 3, 101, 204, 129]],
            &[&[0, 1, 0, 0, 0, 3, 101, 204, 129]],
        ],
    );
}

#[test]
fn should_truncate_null_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES ((NULL)::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-null_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_truncate_char_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('a'::CHAR(6))::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-char_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [&[&[0, 1, 0, 0, 0, 1, 97]], &[&[0, 1, 0, 0, 0, 1, 97]]],
    );
}

#[test]
fn should_truncate_char_long_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('abcdef'::CHAR(8))::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-char_long_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
        ],
    );
}

#[test]
fn should_truncate_integer_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES ((123456::BIGINT)::VARCHAR(3))";
    // Act
    let batches = planning::pruned("varchar-integer_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 50, 51]],
            &[&[0, 1, 0, 0, 0, 3, 49, 50, 51]],
        ],
    );
}

#[test]
fn should_truncate_negative_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (((-123456)::BIGINT)::VARCHAR(4))";
    // Act
    let batches = planning::pruned("varchar-negative_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 8),
        [
            &[&[0, 1, 0, 0, 0, 4, 45, 49, 50, 51]],
            &[&[0, 1, 0, 0, 0, 4, 45, 49, 50, 51]],
        ],
    );
}

#[test]
fn should_truncate_boolean_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES ((true)::VARCHAR(2))";
    // Act
    let batches = planning::pruned("varchar-boolean_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 6),
        [
            &[&[0, 1, 0, 0, 0, 2, 116, 114]],
            &[&[0, 1, 0, 0, 0, 2, 116, 114]],
        ],
    );
}

#[test]
fn should_truncate_float_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES ((1.2345::FLOAT8)::VARCHAR(4))";
    // Act
    let batches = planning::pruned("varchar-float_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 8),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 46, 50, 51]],
            &[&[0, 1, 0, 0, 0, 4, 49, 46, 50, 51]],
        ],
    );
}

#[test]
fn should_truncate_time_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('12:34:56.120000'::TIME)::VARCHAR(7))";
    // Act
    let batches = planning::pruned("varchar-time_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 11),
        [
            &[&[0, 1, 0, 0, 0, 7, 49, 50, 58, 51, 52, 58, 53]],
            &[&[0, 1, 0, 0, 0, 7, 49, 50, 58, 51, 52, 58, 53]],
        ],
    );
}

#[test]
fn should_truncate_timestamp_given_explicit_varchar_when_values_is_executed() {
    // Arrange
    let sql = "VALUES (('2024-02-29 12:34:56.120000'::TIMESTAMP)::VARCHAR(12))";
    // Act
    let batches = planning::pruned("varchar-timestamp_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 16),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 12, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49,
            ]],
        ],
    );
}
