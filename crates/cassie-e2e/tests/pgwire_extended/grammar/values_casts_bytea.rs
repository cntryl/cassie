//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_bytea_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "VALUES ('\\x00Abff'::BYTEA),('\\x00Abff'::BYTEA),('\\x0102'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_repeat_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('\\x00Abff'::BYTEA),('\\x00Abff'::BYTEA),('\\x0102'::BYTEA)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x00Abff'::BYTEA),('\\x00Abff'::BYTEA),('\\x0102'::BYTEA)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),('\\x00Abff'::BYTEA),('\\x0102'::BYTEA) UNION ALL VALUES (NULL::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_repeat_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),(NULL),('\\x0102'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_null_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('\\x00Abff'::BYTEA),(NULL),('\\x0102'::BYTEA)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x00Abff'::BYTEA),(NULL),('\\x0102'::BYTEA)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),(NULL),('\\x0102'::BYTEA) UNION ALL VALUES (NULL::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_null_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x0102'),('\\x00Abff'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_head_standalone", sql);
    // Assert
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
fn should_preserve_bytea_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES ('\\x0102'),('\\x00Abff'::BYTEA)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
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
fn should_preserve_bytea_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x0102'),('\\x00Abff'::BYTEA)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
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
fn should_preserve_bytea_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x0102'),('\\x00Abff'::BYTEA) UNION ALL VALUES (NULL::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_head_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),('\\x0102') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_tail_standalone", sql);
    // Assert
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
fn should_preserve_bytea_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES ('\\x00Abff'::BYTEA),('\\x0102')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
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
fn should_preserve_bytea_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x00Abff'::BYTEA),('\\x0102')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
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
fn should_preserve_bytea_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),('\\x0102') UNION ALL VALUES (NULL::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_unknown_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::BYTEA),('\\x00Abff'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES (NULL::BYTEA),('\\x00Abff'::BYTEA)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::BYTEA),('\\x00Abff'::BYTEA)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::BYTEA),('\\x00Abff'::BYTEA) UNION ALL VALUES (NULL::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_bytea_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),('\\x0102'::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_mixed_width_standalone", sql);
    // Assert
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
fn should_preserve_bytea_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('\\x00Abff'::BYTEA),('\\x0102'::BYTEA)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
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
fn should_preserve_bytea_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x00Abff'::BYTEA),('\\x0102'::BYTEA)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
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
fn should_preserve_bytea_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('\\x00Abff'::BYTEA),('\\x0102'::BYTEA) UNION ALL VALUES (NULL::BYTEA) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-bytea_mixed_width_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
                &[0, 1, 0, 0, 0, 6, 92, 120, 48, 49, 48, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 0, 171, 255],
                &[0, 1, 0, 0, 0, 2, 1, 2],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
