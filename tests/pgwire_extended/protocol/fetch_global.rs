//! UNION and nested FETCH clauses retain their query scope.
use crate::support_bind_planning as planning;
use crate::support_fetch;

#[test]
fn should_preserve_local_right_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL (SELECT 8::INT AS n FETCH FIRST 0 ROWS ONLY)";
    // Act
    let batches = planning::pruned("fetch-global-local_right", sql);
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
fn should_preserve_local_left_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "(SELECT 7::INT AS n FETCH FIRST 0 ROWS ONLY) UNION ALL SELECT 8::INT AS n";
    // Act
    let batches = planning::pruned("fetch-global-local_left", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_local_bound_scope_given_nested_fetch_counts_when_the_set_query_is_limited() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL (SELECT 8::INT AS n FETCH FIRST ROW ONLY) ORDER BY n FETCH FIRST ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-global-global_and_local", sql);
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
fn should_preserve_derived_right_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL SELECT n FROM (SELECT 8::INT AS n FETCH FIRST 0 ROWS ONLY) AS q ORDER BY n FETCH FIRST ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-global-derived_right", sql);
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
fn should_preserve_zero_global_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL SELECT 8::INT AS n ORDER BY n FETCH NEXT 0 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-global-zero_global", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_offset_global_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "SELECT 7::INT AS n UNION ALL SELECT 8::INT AS n FETCH FIRST ROW ONLY OFFSET 1 ROW";
    // Act
    let batches = planning::pruned("fetch-global-offset_global", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_nested_cte_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "WITH q AS (SELECT 7::INT AS n UNION ALL SELECT 8::INT AS n FETCH FIRST ROW ONLY) SELECT * FROM q";
    // Act
    let batches = planning::pruned("fetch-global-nested_cte", sql);
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
fn should_preserve_exists_zero_given_fetch_clauses_when_set_query_scopes_are_bound() {
    // Arrange
    let sql = "SELECT 7::INT WHERE EXISTS (SELECT 8::INT FETCH NEXT 0 ROWS ONLY)";
    // Act
    let batches = planning::pruned("fetch-global-exists_zero", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}
