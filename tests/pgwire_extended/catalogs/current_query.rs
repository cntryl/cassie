//! Exact PostgreSQL 18.6 statement text, descriptors and wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_report_bare_direct_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT current_query() AS output";
    // Act
    let batches = planning::pruned("query-bare_direct", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_spaces_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "  SELECT   current_query() AS output  ";
    // Act
    let batches = planning::pruned("query-bare_spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 38, 32, 32, 83, 69, 76, 69, 67, 84, 32, 32, 32, 99, 117, 114, 114,
                101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116,
                112, 117, 116, 32, 32,
            ]],
            &[&[
                0, 1, 0, 0, 0, 38, 32, 32, 83, 69, 76, 69, 67, 84, 32, 32, 32, 99, 117, 114, 114,
                101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116,
                112, 117, 116, 32, 32,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_comments_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "/* source α */ SELECT current_query() AS output -- tail\n";
    // Act
    let batches = planning::pruned("query-bare_comments", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 57, 47, 42, 32, 115, 111, 117, 114, 99, 101, 32, 206, 177, 32, 42,
                47, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117,
                101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 45, 45,
                32, 116, 97, 105, 108, 10,
            ]],
            &[&[
                0, 1, 0, 0, 0, 57, 47, 42, 32, 115, 111, 117, 114, 99, 101, 32, 206, 177, 32, 42,
                47, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117,
                101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 45, 45,
                32, 116, 97, 105, 108, 10,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_cte_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT current_query() AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("query-bare_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 65, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67,
                84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                65, 83, 32, 111, 117, 116, 112, 117, 116, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111,
                117, 116, 112, 117, 116, 32, 70, 82, 79, 77, 32, 113,
            ]],
            &[&[
                0, 1, 0, 0, 0, 65, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67,
                84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                65, 83, 32, 111, 117, 116, 112, 117, 116, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111,
                117, 116, 112, 117, 116, 32, 70, 82, 79, 77, 32, 113,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_derived_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT current_query() AS output) q";
    // Act
    let batches = planning::pruned("query-bare_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 55, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116, 112, 117, 116, 32,
                70, 82, 79, 77, 32, 40, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116, 41, 32, 113,
            ]],
            &[&[
                0, 1, 0, 0, 0, 55, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116, 112, 117, 116, 32,
                70, 82, 79, 77, 32, 40, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116, 41, 32, 113,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_lower_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT lower(current_query()) AS output";
    // Act
    let batches = planning::pruned("query-bare_lower", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 39, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40,
                99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97,
                115, 32, 111, 117, 116, 112, 117, 116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 39, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40,
                99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97,
                115, 32, 111, 117, 116, 112, 117, 116,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_length_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT length(current_query()) AS output";
    // Act
    let batches = planning::pruned("query-bare_length", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 52, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 40]],
        ],
    );
}

#[test]
fn should_report_bare_coalesce_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT coalesce(current_query(),'missing') AS output";
    // Act
    let batches = planning::pruned("query-bare_coalesce", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 52, 83, 69, 76, 69, 67, 84, 32, 99, 111, 97, 108, 101, 115, 99, 101,
                40, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 44, 39,
                109, 105, 115, 115, 105, 110, 103, 39, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 52, 83, 69, 76, 69, 67, 84, 32, 99, 111, 97, 108, 101, 115, 99, 101,
                40, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 44, 39,
                109, 105, 115, 115, 105, 110, 103, 39, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_empty_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT current_query() AS output WHERE false";
    // Act
    let batches = planning::pruned("query-bare_empty", sql);
    // Assert
    support_values_casts::assert_result(&batches, ("output", 25, -1, -1), [&[], &[]]);
}

#[test]
fn should_report_bare_predicate_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT current_query() AS output WHERE length(current_query()) > 0";
    // Act
    let batches = planning::pruned("query-bare_predicate", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 66, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                32, 87, 72, 69, 82, 69, 32, 108, 101, 110, 103, 116, 104, 40, 99, 117, 114, 114,
                101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 62, 32, 48,
            ]],
            &[&[
                0, 1, 0, 0, 0, 66, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                32, 87, 72, 69, 82, 69, 32, 108, 101, 110, 103, 116, 104, 40, 99, 117, 114, 114,
                101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 62, 32, 48,
            ]],
        ],
    );
}

#[test]
fn should_report_bare_union_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT current_query() AS output UNION ALL SELECT current_query() AS output";
    // Act
    let batches = planning::pruned("query-bare_union", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 75, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                    116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112,
                    117, 116, 32, 85, 78, 73, 79, 78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84,
                    32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                    65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
                &[
                    0, 1, 0, 0, 0, 75, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                    116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112,
                    117, 116, 32, 85, 78, 73, 79, 78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84,
                    32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                    65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 75, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                    116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112,
                    117, 116, 32, 85, 78, 73, 79, 78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84,
                    32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                    65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
                &[
                    0, 1, 0, 0, 0, 75, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                    116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112,
                    117, 116, 32, 85, 78, 73, 79, 78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84,
                    32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                    65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
            ],
        ],
    );
}

#[test]
fn should_report_bare_cast_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT current_query()::TEXT AS output";
    // Act
    let batches = planning::pruned("query-bare_cast", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 38, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 58, 58, 84, 69, 88, 84, 32, 65, 83, 32, 111,
                117, 116, 112, 117, 116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 38, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 58, 58, 84, 69, 88, 84, 32, 65, 83, 32, 111,
                117, 116, 112, 117, 116,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_direct_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.current_query() AS output";
    // Act
    let batches = planning::pruned("query-qualified_direct", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 43, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108,
                111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40,
                41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 43, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108,
                111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40,
                41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_spaces_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "  SELECT   pg_catalog.current_query() AS output  ";
    // Act
    let batches = planning::pruned("query-qualified_spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 49, 32, 32, 83, 69, 76, 69, 67, 84, 32, 32, 32, 112, 103, 95, 99,
                97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117,
                101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 32,
            ]],
            &[&[
                0, 1, 0, 0, 0, 49, 32, 32, 83, 69, 76, 69, 67, 84, 32, 32, 32, 112, 103, 95, 99,
                97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117,
                101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 32,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_comments_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "/* source α */ SELECT pg_catalog.current_query() AS output -- tail\n";
    // Act
    let batches = planning::pruned("query-qualified_comments", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 68, 47, 42, 32, 115, 111, 117, 114, 99, 101, 32, 206, 177, 32, 42,
                47, 32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103,
                46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65,
                83, 32, 111, 117, 116, 112, 117, 116, 32, 45, 45, 32, 116, 97, 105, 108, 10,
            ]],
            &[&[
                0, 1, 0, 0, 0, 68, 47, 42, 32, 115, 111, 117, 114, 99, 101, 32, 206, 177, 32, 42,
                47, 32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103,
                46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65,
                83, 32, 111, 117, 116, 112, 117, 116, 32, 45, 45, 32, 116, 97, 105, 108, 10,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_cte_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_catalog.current_query() AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("query-qualified_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 76, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67,
                84, 32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101,
                110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112,
                117, 116, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116, 112, 117, 116, 32, 70,
                82, 79, 77, 32, 113,
            ]],
            &[&[
                0, 1, 0, 0, 0, 76, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67,
                84, 32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101,
                110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112,
                117, 116, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116, 112, 117, 116, 32, 70,
                82, 79, 77, 32, 113,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_derived_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.current_query() AS output) q";
    // Act
    let batches = planning::pruned("query-qualified_derived", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 66, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116, 112, 117, 116, 32,
                70, 82, 79, 77, 32, 40, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97,
                108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121,
                40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 41, 32, 113,
            ]],
            &[&[
                0, 1, 0, 0, 0, 66, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116, 112, 117, 116, 32,
                70, 82, 79, 77, 32, 40, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97,
                108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121,
                40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 41, 32, 113,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_lower_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT lower(pg_catalog.current_query()) AS output";
    // Act
    let batches = planning::pruned("query-qualified_lower", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 50, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40,
                112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97, 115, 32, 111, 117, 116, 112, 117,
                116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 50, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40,
                112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97, 115, 32, 111, 117, 116, 112, 117,
                116,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_length_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT length(pg_catalog.current_query()) AS output";
    // Act
    let batches = planning::pruned("query-qualified_length", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 53, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 51]],
        ],
    );
}

#[test]
fn should_report_qualified_coalesce_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT coalesce(pg_catalog.current_query(),'missing') AS output";
    // Act
    let batches = planning::pruned("query-qualified_coalesce", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 63, 83, 69, 76, 69, 67, 84, 32, 99, 111, 97, 108, 101, 115, 99, 101,
                40, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 44, 39, 109, 105, 115, 115, 105, 110,
                103, 39, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 63, 83, 69, 76, 69, 67, 84, 32, 99, 111, 97, 108, 101, 115, 99, 101,
                40, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 44, 39, 109, 105, 115, 115, 105, 110,
                103, 39, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_empty_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.current_query() AS output WHERE false";
    // Act
    let batches = planning::pruned("query-qualified_empty", sql);
    // Assert
    support_values_casts::assert_result(&batches, ("output", 25, -1, -1), [&[], &[]]);
}

#[test]
fn should_report_qualified_predicate_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql =
        "SELECT pg_catalog.current_query() AS output WHERE length(pg_catalog.current_query()) > 0";
    // Act
    let batches = planning::pruned("query-qualified_predicate", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 88, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108,
                111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40,
                41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 87, 72, 69, 82, 69, 32, 108,
                101, 110, 103, 116, 104, 40, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99,
                117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 62, 32,
                48,
            ]],
            &[&[
                0, 1, 0, 0, 0, 88, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108,
                111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40,
                41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 87, 72, 69, 82, 69, 32, 108,
                101, 110, 103, 116, 104, 40, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99,
                117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 62, 32,
                48,
            ]],
        ],
    );
}

#[test]
fn should_report_qualified_union_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.current_query() AS output UNION ALL SELECT pg_catalog.current_query() AS output";
    // Act
    let batches = planning::pruned("query-qualified_union", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 97, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97,
                    108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114,
                    121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 85, 78, 73, 79,
                    78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116,
                    97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101,
                    114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
                &[
                    0, 1, 0, 0, 0, 97, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97,
                    108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114,
                    121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 85, 78, 73, 79,
                    78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116,
                    97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101,
                    114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
            ],
            &[
                &[
                    0, 1, 0, 0, 0, 97, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97,
                    108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114,
                    121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 85, 78, 73, 79,
                    78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116,
                    97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101,
                    114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
                &[
                    0, 1, 0, 0, 0, 97, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97,
                    108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114,
                    121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 85, 78, 73, 79,
                    78, 32, 65, 76, 76, 32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116,
                    97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101,
                    114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                ],
            ],
        ],
    );
}

#[test]
fn should_report_qualified_cast_given_prepared_sql_when_current_query_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.current_query()::TEXT AS output";
    // Act
    let batches = planning::pruned("query-qualified_cast", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 49, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108,
                111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40,
                41, 58, 58, 84, 69, 88, 84, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
            &[&[
                0, 1, 0, 0, 0, 49, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108,
                111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40,
                41, 58, 58, 84, 69, 88, 84, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            ]],
        ],
    );
}
