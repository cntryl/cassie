//! PostgreSQL FETCH/OFFSET offsets controls.
use crate::support_bind_planning as planning;
use crate::support_fetch;

#[test]
fn should_preserve_offset_before_row_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 0 ROW FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-offset_before_row_0", sql);
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
fn should_preserve_offset_before_row_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 1 ROW FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-offset_before_row_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56], &[0, 1, 0, 0, 0, 1, 57]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_offset_before_row_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET NULL ROW FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-offset_before_row_null", sql);
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
fn should_preserve_offset_before_rows_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 0 ROWS FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-offset_before_rows_0", sql);
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
fn should_preserve_offset_before_rows_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 1 ROWS FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-offset_before_rows_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56], &[0, 1, 0, 0, 0, 1, 57]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_offset_before_rows_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET NULL ROWS FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-offset_before_rows_null", sql);
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
fn should_preserve_offset_after_row_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY OFFSET 0 ROW";
    // Act
    let batches = planning::pruned("fetch-offset_after_row_0", sql);
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
fn should_preserve_offset_after_row_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY OFFSET 1 ROW";
    // Act
    let batches = planning::pruned("fetch-offset_after_row_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56], &[0, 1, 0, 0, 0, 1, 57]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_offset_after_row_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY OFFSET NULL ROW";
    // Act
    let batches = planning::pruned("fetch-offset_after_row_null", sql);
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
fn should_preserve_offset_after_rows_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY OFFSET 0 ROWS";
    // Act
    let batches = planning::pruned("fetch-offset_after_rows_0", sql);
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
fn should_preserve_offset_after_rows_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY OFFSET 1 ROWS";
    // Act
    let batches = planning::pruned("fetch-offset_after_rows_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56], &[0, 1, 0, 0, 0, 1, 57]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_offset_after_rows_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY OFFSET NULL ROWS";
    // Act
    let batches = planning::pruned("fetch-offset_after_rows_null", sql);
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
