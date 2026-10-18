//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_varchar_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "VALUES ('ab🙂'::VARCHAR(4)),('ab🙂'::VARCHAR(4)),('b'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_repeat_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('ab🙂'::VARCHAR(4)),('ab🙂'::VARCHAR(4)),('b'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('ab🙂'::VARCHAR(4)),('ab🙂'::VARCHAR(4)),('b'::VARCHAR(4))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),('ab🙂'::VARCHAR(4)),('b'::VARCHAR(4)) UNION ALL VALUES (NULL::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_repeat_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),(NULL),('b'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_null_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('ab🙂'::VARCHAR(4)),(NULL),('b'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('ab🙂'::VARCHAR(4)),(NULL),('b'::VARCHAR(4))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),(NULL),('b'::VARCHAR(4)) UNION ALL VALUES (NULL::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_null_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('b'),('ab🙂'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_head_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('b'),('ab🙂'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "WITH q(v) AS (VALUES ('b'),('ab🙂'::VARCHAR(4))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('b'),('ab🙂'::VARCHAR(4)) UNION ALL VALUES (NULL::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_head_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),('b') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('ab🙂'::VARCHAR(4)),('b')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "WITH q(v) AS (VALUES ('ab🙂'::VARCHAR(4)),('b')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),('b') UNION ALL VALUES (NULL::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_unknown_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::VARCHAR(4)),('ab🙂'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES (NULL::VARCHAR(4)),('ab🙂'::VARCHAR(4))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::VARCHAR(4)),('ab🙂'::VARCHAR(4))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::VARCHAR(4)),('ab🙂'::VARCHAR(4)) UNION ALL VALUES (NULL::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, 8),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),('b'::VARCHAR(7)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_mixed_width_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('ab🙂'::VARCHAR(4)),('b'::VARCHAR(7))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('ab🙂'::VARCHAR(4)),('b'::VARCHAR(7))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('ab🙂'::VARCHAR(4)),('b'::VARCHAR(7)) UNION ALL VALUES (NULL::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-varchar_mixed_width_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 98, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
