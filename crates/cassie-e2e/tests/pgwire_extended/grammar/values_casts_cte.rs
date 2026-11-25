//! Adversarial character ownership across CTE boundaries.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_cte_distinct_given_character_values_when_cte_materialization_runs() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a'::CHAR(3)),('a'::CHAR(5))) SELECT COUNT(*)::INT AS result FROM (SELECT DISTINCT v FROM q) d";
    // Act
    let batches = planning::pruned("valuescasts-final-cte_distinct", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_cte_text_given_character_values_when_cte_materialization_runs() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a'::CHAR(3)),('long🙂 ')) SELECT v::TEXT AS result FROM q ORDER BY v";
    // Act
    let batches = planning::pruned("valuescasts-final-cte_text", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 25, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
        ],
    );
}

#[test]
fn should_preserve_cte_raw_given_character_values_when_cte_materialization_runs() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a'::CHAR(3)),('long🙂 ')) SELECT v FROM q ORDER BY v";
    // Act
    let batches = planning::pruned("valuescasts-final-cte_raw", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_cte_widths_given_character_values_when_cte_materialization_runs() {
    // Arrange
    let sql =
        "WITH q(k,v) AS (VALUES (1,'a'::CHAR(3)),(2,'a'::CHAR(5))) SELECT v FROM q ORDER BY k";
    // Act
    let batches = planning::pruned("valuescasts-final-cte_widths", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 97, 32, 32, 32, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 5, 97, 32, 32, 32, 32],
            ],
        ],
    );
}
