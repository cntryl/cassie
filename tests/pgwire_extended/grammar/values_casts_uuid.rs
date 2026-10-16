//! Native typed VALUES payloads and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_uuid_repeat_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_repeat_standalone", sql);
    // Assert
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
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_repeat_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_repeat_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_repeat_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_repeat_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_repeat_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID) UNION ALL VALUES (NULL::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_repeat_set", sql);
    // Assert
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
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_null_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),(NULL),('00000000-0000-0000-0000-000000000001'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_null_tail_standalone", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_null_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),(NULL),('00000000-0000-0000-0000-000000000001'::UUID)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_null_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_null_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),(NULL),('00000000-0000-0000-0000-000000000001'::UUID)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_null_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_null_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),(NULL),('00000000-0000-0000-0000-000000000001'::UUID) UNION ALL VALUES (NULL::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_null_tail_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_unknown_head_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('00000000-0000-0000-0000-000000000001'),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_head_standalone", sql);
    // Assert
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
fn should_preserve_uuid_unknown_head_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('00000000-0000-0000-0000-000000000001'),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_head_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
fn should_preserve_uuid_unknown_head_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('00000000-0000-0000-0000-000000000001'),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_head_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
fn should_preserve_uuid_unknown_head_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('00000000-0000-0000-0000-000000000001'),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID) UNION ALL VALUES (NULL::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_head_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_unknown_tail_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_tail_standalone", sql);
    // Assert
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
fn should_preserve_uuid_unknown_tail_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001')) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_tail_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
fn should_preserve_uuid_unknown_tail_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001')) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_tail_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
fn should_preserve_uuid_unknown_tail_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001') UNION ALL VALUES (NULL::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_unknown_tail_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_typed_null_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql =
        "VALUES (NULL::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_typed_null_standalone", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 2950, 16, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_typed_null_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES (NULL::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_typed_null_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_typed_null_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (NULL::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_typed_null_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_typed_null_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES (NULL::UUID),('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID) UNION ALL VALUES (NULL::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_typed_null_set", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 2950, 16, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45,
                    52, 101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48,
                    97, 49, 49,
                ],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_uuid_mixed_width_standalone_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_mixed_width_standalone", sql);
    // Assert
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
fn should_preserve_uuid_mixed_width_derived_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "SELECT v FROM (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_mixed_width_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
fn should_preserve_uuid_mixed_width_cte_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID)) SELECT v FROM q ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_mixed_width_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 2950, 16, -1),
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
fn should_preserve_uuid_mixed_width_set_given_typed_values_when_rows_are_described() {
    // Arrange
    let sql = "VALUES ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID),('00000000-0000-0000-0000-000000000001'::UUID) UNION ALL VALUES (NULL::UUID) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-uuid_mixed_width_set", sql);
    // Assert
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
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ],
                &[
                    0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185,
                    189, 56, 10, 17,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
