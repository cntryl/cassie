//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_timestamp_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_repeat_standalone", sql);
    // Assert
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
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP) UNION ALL VALUES (NULL::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_repeat_set", sql);
    // Assert
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
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),(NULL),('2000-01-01 00:00:00'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_null_tail_standalone", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),(NULL),('2000-01-01 00:00:00'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),(NULL),('2000-01-01 00:00:00'::TIMESTAMP)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),(NULL),('2000-01-01 00:00:00'::TIMESTAMP) UNION ALL VALUES (NULL::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_null_tail_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2000-01-01 00:00:00'),('2024-02-29 12:34:56.120000'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_head_standalone", sql);
    // Assert
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
fn should_preserve_timestamp_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2000-01-01 00:00:00'),('2024-02-29 12:34:56.120000'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
fn should_preserve_timestamp_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2000-01-01 00:00:00'),('2024-02-29 12:34:56.120000'::TIMESTAMP)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
fn should_preserve_timestamp_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2000-01-01 00:00:00'),('2024-02-29 12:34:56.120000'::TIMESTAMP) UNION ALL VALUES (NULL::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_head_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_tail_standalone", sql);
    // Assert
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
fn should_preserve_timestamp_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
fn should_preserve_timestamp_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
fn should_preserve_timestamp_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00') UNION ALL VALUES (NULL::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_unknown_tail_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "VALUES (NULL::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1114, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES (NULL::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::TIMESTAMP),('2024-02-29 12:34:56.120000'::TIMESTAMP) UNION ALL VALUES (NULL::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1114, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 22, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57, 32, 49, 50, 58, 51,
                    52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_timestamp_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_mixed_width_standalone", sql);
    // Assert
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
fn should_preserve_timestamp_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
fn should_preserve_timestamp_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1114, 8, -1),
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
fn should_preserve_timestamp_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29 12:34:56.120000'::TIMESTAMP),('2000-01-01 00:00:00'::TIMESTAMP) UNION ALL VALUES (NULL::TIMESTAMP) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-timestamp_mixed_width_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 2, 181, 131, 65, 104, 80, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
