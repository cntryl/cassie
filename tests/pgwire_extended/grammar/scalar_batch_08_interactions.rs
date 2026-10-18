//! Native LIKE ESCAPE row, coercion, folding and query-context witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_like_interaction_0_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v LIKE 'a!_b' ESCAPE '!' AS output FROM (VALUES('a_b'),('axb'),(NULL::TEXT)) AS t(v)";
    // Act
    let batches = planning::pruned("like-interaction-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_like_interaction_1_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql =
        "SELECT NULL::TEXT LIKE p ESCAPE '!!' AS output FROM (VALUES('x'),(NULL::TEXT)) AS t(p)";
    // Act
    let batches = planning::pruned("like-interaction-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_like_interaction_2_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v LIKE p ESCAPE e AS output FROM (VALUES('a_b','a!_b','!'),('axb','a!_b','!'),(NULL::TEXT,NULL::TEXT,'!!')) AS t(v,p,e)";
    // Act
    let batches = planning::pruned("like-interaction-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_like_interaction_3_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 'a' LIKE 'a'::CHAR(3) ESCAPE '!'::CHAR(3) AS output";
    // Act
    let batches = planning::pruned("like-interaction-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_like_interaction_4_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3) LIKE 'a' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("like-interaction-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_like_interaction_5_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3) LIKE 'a%' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("like-interaction-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_like_interaction_6_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql =
        "SELECT v FROM (VALUES('a_b'),('axb'),(NULL::TEXT)) AS t(v) WHERE v LIKE 'a!_b' ESCAPE '!'";
    // Act
    let batches = planning::pruned("like-interaction-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 95, 98]],
            &[&[0, 1, 0, 0, 0, 3, 97, 95, 98]],
        ],
    );
}

#[test]
fn should_preserve_like_interaction_7_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "WITH t AS (SELECT 'a_b' AS v) SELECT v LIKE 'a!_b' ESCAPE '!' AS output FROM t";
    // Act
    let batches = planning::pruned("like-interaction-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_like_interaction_8_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN 'a_b' LIKE 'a!_b' ESCAPE '!' THEN 7 ELSE 8 END AS output";
    // Act
    let batches = planning::pruned("like-interaction-8", sql);
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
fn should_preserve_like_interaction_9_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT EXISTS(SELECT 1 WHERE 'a_b' LIKE 'a!_b' ESCAPE '!') AS output";
    // Act
    let batches = planning::pruned("like-interaction-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_like_interaction_10_given_typed_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE CASE WHEN EXISTS(SELECT 1) THEN 'a!_b' ELSE 'x' END ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("like-interaction-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
