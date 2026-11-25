//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_char_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('a'::CHAR(3)),('b'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_repeat_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, 7),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('a'::CHAR(3)),('a'::CHAR(3)),('b'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, 7),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a'::CHAR(3)),('a'::CHAR(3)),('b'::CHAR(3))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, 7),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('a'::CHAR(3)),('b'::CHAR(3)) UNION ALL VALUES (NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_repeat_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, 7),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),(NULL),('b'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_null_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES ('a'::CHAR(3)),(NULL),('b'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a'::CHAR(3)),(NULL),('b'::CHAR(3))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),(NULL),('b'::CHAR(3)) UNION ALL VALUES (NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_null_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 3, 98, 32, 32],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('b'),('a'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_head_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('b'),('a'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('b'),('a'::CHAR(3))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('b'),('a'::CHAR(3)) UNION ALL VALUES (NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_head_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('b') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('a'::CHAR(3)),('b')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a'::CHAR(3)),('b')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_preserve_char_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('b') UNION ALL VALUES (NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_unknown_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 1, 98],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::CHAR(3)),('a'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_char_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES (NULL::CHAR(3)),('a'::CHAR(3))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_char_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::CHAR(3)),('a'::CHAR(3))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_char_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::CHAR(3)),('a'::CHAR(3)) UNION ALL VALUES (NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, 7),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('b'::CHAR(5)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_mixed_width_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('a'::CHAR(3)),('b'::CHAR(5))) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "WITH q(v) AS (VALUES ('a'::CHAR(3)),('b'::CHAR(5))) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('b'::CHAR(5)) UNION ALL VALUES (NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-char_mixed_width_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 98, 32, 32, 32, 32],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
