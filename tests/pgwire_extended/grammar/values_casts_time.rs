//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_time_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),('12:34:56.120000'::TIME),('00:00:00'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_repeat_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('12:34:56.120000'::TIME),('12:34:56.120000'::TIME),('00:00:00'::TIME)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('12:34:56.120000'::TIME),('12:34:56.120000'::TIME),('00:00:00'::TIME)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),('12:34:56.120000'::TIME),('00:00:00'::TIME) UNION ALL VALUES (NULL::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_repeat_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),(NULL),('00:00:00'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_null_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('12:34:56.120000'::TIME),(NULL),('00:00:00'::TIME)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('12:34:56.120000'::TIME),(NULL),('00:00:00'::TIME)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),(NULL),('00:00:00'::TIME) UNION ALL VALUES (NULL::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_null_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('00:00:00'),('12:34:56.120000'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_head_standalone", sql);
    // Assert
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
fn should_preserve_time_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('00:00:00'),('12:34:56.120000'::TIME)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
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
fn should_preserve_time_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('00:00:00'),('12:34:56.120000'::TIME)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
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
fn should_preserve_time_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('00:00:00'),('12:34:56.120000'::TIME) UNION ALL VALUES (NULL::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_head_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),('00:00:00') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_tail_standalone", sql);
    // Assert
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
fn should_preserve_time_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('12:34:56.120000'::TIME),('00:00:00')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
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
fn should_preserve_time_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('12:34:56.120000'::TIME),('00:00:00')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
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
fn should_preserve_time_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),('00:00:00') UNION ALL VALUES (NULL::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_unknown_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::TIME),('12:34:56.120000'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES (NULL::TIME),('12:34:56.120000'::TIME)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::TIME),('12:34:56.120000'::TIME)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::TIME),('12:34:56.120000'::TIME) UNION ALL VALUES (NULL::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_time_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),('00:00:00'::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_mixed_width_standalone", sql);
    // Assert
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
fn should_preserve_time_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('12:34:56.120000'::TIME),('00:00:00'::TIME)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
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
fn should_preserve_time_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('12:34:56.120000'::TIME),('00:00:00'::TIME)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1083, 8, -1),
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
fn should_preserve_time_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('12:34:56.120000'::TIME),('00:00:00'::TIME) UNION ALL VALUES (NULL::TIME) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-time_mixed_width_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1083, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 48, 48, 58, 48, 48, 58, 48, 48],
                &[
                    0, 1, 0, 0, 0, 11, 49, 50, 58, 51, 52, 58, 53, 54, 46, 49, 50,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 10, 139, 219, 240, 192],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
