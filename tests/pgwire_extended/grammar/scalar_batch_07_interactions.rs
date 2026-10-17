//! Native cast predicate folding, typed rows, filtering and aggregate witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_cast_interaction_0_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT int4('7')";
    // Act
    let batches = planning::pruned("cast-interaction-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("int4", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_1_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql =
        "SELECT date(v) FROM (VALUES ('2001-02-16 20:38:40'::TIMESTAMP),(NULL::TIMESTAMP)) AS t(v)";
    // Act
    let batches = planning::pruned("cast-interaction-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("date", 1082, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 10, 50, 48, 48, 49, 45, 48, 50, 45, 49, 54],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 1, 156],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_2_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT text(v) FROM (VALUES(1),(2),(NULL::INTEGER)) AS t(v)";
    // Act
    let batches = planning::pruned("cast-interaction-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("text", 25, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_3_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "WITH c AS (SELECT int4('7') AS v) SELECT v+1 AS output FROM c";
    // Act
    let batches = planning::pruned("cast-interaction-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_4_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "WITH c AS (SELECT float8('1.5') AS v) SELECT v^2 AS output FROM c";
    // Act
    let batches = planning::pruned("cast-interaction-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 50, 46, 50, 53]],
            &[&[0, 1, 0, 0, 0, 8, 64, 2, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_5_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT int4(v) AS output FROM (VALUES('7'::TEXT),('8'::TEXT),(NULL::TEXT)) AS t(v)";
    // Act
    let batches = planning::pruned("cast-interaction-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 55],
                &[0, 1, 0, 0, 0, 1, 56],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_6_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN TRUE THEN int4('7') ELSE 1/0 END AS output";
    // Act
    let batches = planning::pruned("cast-interaction-6", sql);
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
fn should_preserve_cast_interaction_7_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT int4('7') BETWEEN SYMMETRIC 3 AND 8 AS output";
    // Act
    let batches = planning::pruned("cast-interaction-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_cast_interaction_8_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT text(EXISTS(SELECT 1)) AS output";
    // Act
    let batches = planning::pruned("cast-interaction-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 116, 114, 117, 101]],
            &[&[0, 1, 0, 0, 0, 4, 116, 114, 117, 101]],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_9_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT int4('7')+int4('8') AS output";
    // Act
    let batches = planning::pruned("cast-interaction-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 15]],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_10_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT int4('7') AS output FROM (VALUES('yes'::TEXT),('off'::TEXT),(NULL::TEXT)) AS t(v) WHERE bool(v)";
    // Act
    let batches = planning::pruned("cast-interaction-10", sql);
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
fn should_preserve_cast_interaction_11_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.time('20:38:40')";
    // Act
    let batches = planning::pruned("cast-interaction-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("time", 1083, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 50, 48, 58, 51, 56, 58, 52, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 17, 77, 209, 52, 0]],
        ],
    );
}

#[test]
fn should_preserve_cast_interaction_12_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.varchar('café')";
    // Act
    let batches = planning::pruned("cast-interaction-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("varchar", 1043, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 99, 97, 102, 195, 169]],
            &[&[0, 1, 0, 0, 0, 5, 99, 97, 102, 195, 169]],
        ],
    );
}
