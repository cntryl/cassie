//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_date_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),('2024-02-29'::DATE),('2000-01-01'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_repeat_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29'::DATE),('2024-02-29'::DATE),('2000-01-01'::DATE)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29'::DATE),('2024-02-29'::DATE),('2000-01-01'::DATE)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),('2024-02-29'::DATE),('2000-01-01'::DATE) UNION ALL VALUES (NULL::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_repeat_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),(NULL),('2000-01-01'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_null_tail_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29'::DATE),(NULL),('2000-01-01'::DATE)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29'::DATE),(NULL),('2000-01-01'::DATE)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),(NULL),('2000-01-01'::DATE) UNION ALL VALUES (NULL::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_null_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2000-01-01'),('2024-02-29'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_head_standalone", sql);
    // Assert
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
fn should_preserve_date_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES ('2000-01-01'),('2024-02-29'::DATE)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
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
fn should_preserve_date_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2000-01-01'),('2024-02-29'::DATE)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
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
fn should_preserve_date_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2000-01-01'),('2024-02-29'::DATE) UNION ALL VALUES (NULL::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_head_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),('2000-01-01') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_tail_standalone", sql);
    // Assert
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
fn should_preserve_date_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES ('2024-02-29'::DATE),('2000-01-01')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
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
fn should_preserve_date_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29'::DATE),('2000-01-01')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
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
fn should_preserve_date_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),('2000-01-01') UNION ALL VALUES (NULL::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_unknown_tail_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::DATE),('2024-02-29'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES (NULL::DATE),('2024-02-29'::DATE)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::DATE),('2024-02-29'::DATE)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::DATE),('2024-02-29'::DATE) UNION ALL VALUES (NULL::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_date_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),('2000-01-01'::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_mixed_width_standalone", sql);
    // Assert
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
fn should_preserve_date_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('2024-02-29'::DATE),('2000-01-01'::DATE)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
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
fn should_preserve_date_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('2024-02-29'::DATE),('2000-01-01'::DATE)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1082, 4, -1),
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
fn should_preserve_date_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('2024-02-29'::DATE),('2000-01-01'::DATE) UNION ALL VALUES (NULL::DATE) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-date_mixed_width_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
                &[0, 1, 0, 0, 0, 10, 50, 48, 50, 52, 45, 48, 50, 45, 50, 57],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0, 4, 0, 0, 34, 121],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
