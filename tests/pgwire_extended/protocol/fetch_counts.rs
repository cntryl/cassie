//! PostgreSQL FETCH/OFFSET counts controls.
use crate::support_bind_planning as planning;
use crate::support_fetch;

#[test]
fn should_preserve_first_row_default_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-first_row_default", sql);
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
fn should_preserve_first_row_zero_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 0 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-first_row_zero", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_first_row_one_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-first_row_one", sql);
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
fn should_preserve_first_row_two_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-first_row_two", sql);
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
fn should_preserve_first_row_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST NULL ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-first_row_null", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 55],
                &[0, 1, 0, 0, 0, 1, 56],
                &[0, 1, 0, 0, 0, 1, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_first_row_expression_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (1+1) ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-first_row_expression", sql);
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
fn should_preserve_first_rows_default_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-first_rows_default", sql);
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
fn should_preserve_first_rows_zero_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 0 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-first_rows_zero", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_first_rows_one_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-first_rows_one", sql);
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
fn should_preserve_first_rows_two_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-first_rows_two", sql);
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
fn should_preserve_first_rows_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST NULL ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-first_rows_null", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 55],
                &[0, 1, 0, 0, 0, 1, 56],
                &[0, 1, 0, 0, 0, 1, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_first_rows_expression_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (1+1) ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-first_rows_expression", sql);
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
fn should_preserve_next_row_default_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-next_row_default", sql);
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
fn should_preserve_next_row_zero_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 0 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-next_row_zero", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_next_row_one_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 1 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-next_row_one", sql);
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
fn should_preserve_next_row_two_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 2 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-next_row_two", sql);
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
fn should_preserve_next_row_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT NULL ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-next_row_null", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 55],
                &[0, 1, 0, 0, 0, 1, 56],
                &[0, 1, 0, 0, 0, 1, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_next_row_expression_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT (1+1) ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-next_row_expression", sql);
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
fn should_preserve_next_rows_default_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-next_rows_default", sql);
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
fn should_preserve_next_rows_zero_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 0 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-next_rows_zero", sql);
    // Assert
    support_fetch::assert_result(&batches, [&[], &[]]);
}

#[test]
fn should_preserve_next_rows_one_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 1 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-next_rows_one", sql);
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
fn should_preserve_next_rows_two_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 2 ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-next_rows_two", sql);
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
fn should_preserve_next_rows_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT NULL ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-next_rows_null", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 55],
                &[0, 1, 0, 0, 0, 1, 56],
                &[0, 1, 0, 0, 0, 1, 57],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 8],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 9],
            ],
        ],
    );
}

#[test]
fn should_preserve_next_rows_expression_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT (1+1) ROWS ONLY";
    // Act
    let batches = planning::pruned("fetch-next_rows_expression", sql);
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
