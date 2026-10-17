//! Native UTF-8 SQL identifier boundaries over aliases, references and DDL.
use crate::support_bind_planning as planning;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use crate::support_values_casts;

#[test]
fn should_preserve_identifier_0_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"";
    // Act
    let batches = planning::pruned("identifier-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_1_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\") AS names";
    // Act
    let batches = planning::pruned("identifier-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_2_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"";
    // Act
    let batches = planning::pruned("identifier-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_3_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\") AS names";
    // Act
    let batches = planning::pruned("identifier-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_4_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaétail\"";
    // Act
    let batches = planning::pruned("identifier-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_5_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaétail\") AS names";
    // Act
    let batches = planning::pruned("identifier-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_6_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé\"";
    // Act
    let batches = planning::pruned("identifier-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_7_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé\") AS names";
    // Act
    let batches = planning::pruned("identifier-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_8_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€tail\"";
    // Act
    let batches = planning::pruned("identifier-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_9_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€tail\") AS names";
    // Act
    let batches = planning::pruned("identifier-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_10_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€\"";
    // Act
    let batches = planning::pruned("identifier-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_11_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa€\") AS names";
    // Act
    let batches = planning::pruned("identifier-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_12_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂tail\"";
    // Act
    let batches = planning::pruned("identifier-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_13_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂tail\") AS names";
    // Act
    let batches = planning::pruned("identifier-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_14_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂\"";
    // Act
    let batches = planning::pruned("identifier-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_15_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa🙂\") AS names";
    // Act
    let batches = planning::pruned("identifier-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_16_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\"étail\"";
    // Act
    let batches = planning::pruned("identifier-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_17_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\"\" FROM (SELECT 7 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\"étail\") AS names";
    // Act
    let batches = planning::pruned("identifier-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_18_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"ééééééééééééééééééééééééééééééééé\"";
    // Act
    let batches = planning::pruned("identifier-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("ééééééééééééééééééééééééééééééé", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_19_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"ééééééééééééééééééééééééééééééé\" FROM (SELECT 7 AS \"ééééééééééééééééééééééééééééééééé\") AS names";
    // Act
    let batches = planning::pruned("identifier-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("ééééééééééééééééééééééééééééééé", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_20_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS \"🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂\"";
    // Act
    let batches = planning::pruned("identifier-20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_21_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂\" FROM (SELECT 7 AS \"🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂\") AS names";
    // Act
    let batches = planning::pruned("identifier-21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_22_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\".v AS output FROM (SELECT 7 AS v) AS \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\"";
    // Act
    let batches = planning::pruned("identifier-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_23_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "WITH \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\" AS (SELECT 7 AS v) SELECT v AS output FROM \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\"";
    // Act
    let batches = planning::pruned("identifier-23", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_24_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "WITH names(\"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\") AS (SELECT 7) SELECT \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\" FROM names";
    // Act
    let batches = planning::pruned("identifier-24", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_25_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\" FROM (VALUES(7)) AS names(\"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\")";
    // Act
    let batches = planning::pruned("identifier-25", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_26_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\" FROM (SELECT 7 AS \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\") AS names WHERE \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\"=7";
    // Act
    let batches = planning::pruned("identifier-26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_27_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\" FROM (SELECT 7 AS \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\") AS names ORDER BY \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\"";
    // Act
    let batches = planning::pruned("identifier-27", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_28_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT 7 AS ABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABC";
    // Act
    let batches = planning::pruned("identifier-28", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "abcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabc",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_29_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT ABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABC FROM (SELECT 7 AS ABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABC) AS names";
    // Act
    let batches = planning::pruned("identifier-29", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "abcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabc",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_30_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql =
        "SELECT \"v\" FROM \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\"";
    // Act
    let batches = planning::pruned_with_setup("identifier-30", sql, &["CREATE TABLE \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\" (\"v\" INTEGER)","INSERT INTO \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\" VALUES(7)"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_31_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx\" FROM \"i9_table_1\"";
    // Act
    let batches = planning::pruned_with_setup("identifier-31", sql, &["CREATE TABLE \"i9_table_1\" (\"i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\" INTEGER)","INSERT INTO \"i9_table_1\" VALUES(7)"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "i9_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéx",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_32_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM \"i9_table_2\"";
    // Act
    let batches = planning::pruned_with_setup("identifier-32", sql, &["CREATE TABLE \"i9_table_2\" (\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéz\" INTEGER)","INSERT INTO \"i9_table_2\" VALUES(7)"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_identifier_33_given_native_byte_boundary_when_rows_are_described() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\"\" FROM \"i9_table_3\"";
    // Act
    let batches = planning::pruned_with_setup("identifier-33", sql, &["CREATE TABLE \"i9_table_3\" (\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\"étail\" INTEGER)","INSERT INTO \"i9_table_3\" VALUES(7)"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_reject_identifier_collision_0_given_truncated_names_when_execution_is_requested() {
    // Arrange
    let sql = "CREATE TABLE i9_collision (\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" INTEGER, \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaatail\" INTEGER)";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("identifier-collision-0", &[], cycles);
    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42701"));
        assert!(!frames.iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
    }
}

#[test]
fn should_reject_identifier_collision_1_given_truncated_names_when_execution_is_requested() {
    // Arrange
    let sql = "SELECT \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" FROM (SELECT 1 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",2 AS \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaatail\") AS t";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("identifier-collision-1", &[], cycles);
    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42702"));
        assert!(!frames.iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
    }
}

#[test]
fn should_truncate_decoded_unicode_identifier_0_given_escaped_sql_when_output_is_described() {
    // Arrange
    let sql =
        "SELECT 7 AS U&\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\00e9tail\"";
    // Act
    let batches = planning::pruned("identifier-unicode-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_truncate_decoded_unicode_identifier_1_given_escaped_sql_when_output_is_described() {
    // Arrange
    let sql = "SELECT 7 AS U&\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa#00e9tail\" UESCAPE '#'";
    // Act
    let batches = planning::pruned("identifier-unicode-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé",
            23,
            4,
            -1,
        ),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_generated_qualifier_0_given_long_sql_alias_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\".v AS output FROM (VALUES(7)) AS \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"(v)";
    // Act
    let batches = planning::pruned("identifier-internal-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_generated_qualifier_1_given_long_sql_alias_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\".v AS output FROM (VALUES(7)) AS \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"(v) JOIN (VALUES(7)) AS \"i9_right_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"(v) ON \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\".v=\"i9_right_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\".v";
    // Act
    let batches = planning::pruned("identifier-internal-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_generated_qualifier_2_given_long_sql_alias_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\".v AS output FROM (VALUES(7)) AS \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"(v) WHERE \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\".v=7 ORDER BY \"i9_left_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\".v";
    // Act
    let batches = planning::pruned("identifier-internal-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_alias_probe_0_given_delimited_identity_when_long_alias_is_referenced() {
    // Arrange
    let sql = "SELECT \"CaseAlias_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\".v AS output FROM (SELECT 7 AS v) AS \"CaseAlias_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"";
    // Act
    let batches = planning::pruned("identifier-alias-probe-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_alias_probe_1_given_delimited_identity_when_long_alias_is_referenced() {
    // Arrange
    let sql = "SELECT \"Quote\"\"Alias_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\".v AS output FROM (SELECT 7 AS v) AS \"Quote\"\"Alias_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"";
    // Act
    let batches = planning::pruned("identifier-alias-probe-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_alias_probe_2_given_delimited_identity_when_long_alias_is_referenced() {
    // Arrange
    let sql = "SELECT \"Space Alias_ccccccccccccccccccccccccccccccccccccccccccccccccccc\".v AS output FROM (SELECT 7 AS v) AS \"Space Alias_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"";
    // Act
    let batches = planning::pruned("identifier-alias-probe-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_alias_probe_3_given_delimited_identity_when_long_alias_is_referenced() {
    // Arrange
    let sql = "SELECT \"Dot.Alias_ddddddddddddddddddddddddddddddddddddddddddddddddddddd\".v AS output FROM (SELECT 7 AS v) AS \"Dot.Alias_dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\"";
    // Act
    let batches = planning::pruned("identifier-alias-probe-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}
