//! PostgreSQL FETCH/OFFSET contexts controls.
use crate::support_bind_planning as planning;
use crate::support_fetch;

#[test]
fn should_preserve_values_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "VALUES (7::INT),(8::INT),(9::INT) ORDER BY column1 FETCH NEXT 0 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-values_0", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_values_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "VALUES (7::INT),(8::INT),(9::INT) ORDER BY column1 FETCH NEXT 1 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-values_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_values_2_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "VALUES (7::INT),(8::INT),(9::INT) ORDER BY column1 FETCH NEXT 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-values_2", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 1, 56]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
            ],
        ],
    );
}

#[test]
fn should_preserve_derived_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT AS n FETCH NEXT 0 ROW ONLY) AS q";
    // Act
    let batches = planning::pruned("fetch-derived_0", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_derived_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT AS n FETCH NEXT 1 ROW ONLY) AS q";
    // Act
    let batches = planning::pruned("fetch-derived_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_derived_2_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT AS n FETCH NEXT 2 ROW ONLY) AS q";
    // Act
    let batches = planning::pruned("fetch-derived_2", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_cte_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "WITH q AS (SELECT 7::INT AS n FETCH FIRST 0 ROWS ONLY) SELECT * FROM q";
    // Act
    let batches = planning::pruned("fetch-cte_0", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_cte_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "WITH q AS (SELECT 7::INT AS n FETCH FIRST 1 ROWS ONLY) SELECT * FROM q";
    // Act
    let batches = planning::pruned("fetch-cte_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_cte_2_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "WITH q AS (SELECT 7::INT AS n FETCH FIRST 2 ROWS ONLY) SELECT * FROM q";
    // Act
    let batches = planning::pruned("fetch-cte_2", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_union_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL SELECT 8::INT AS n ORDER BY n FETCH NEXT 0 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-union_0", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_union_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL SELECT 8::INT AS n ORDER BY n FETCH NEXT 1 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-union_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_union_2_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL SELECT 8::INT AS n ORDER BY n FETCH NEXT 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-union_2", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 1, 56]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
            ],
        ],
    );
}
