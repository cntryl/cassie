//! PostgreSQL FETCH/OFFSET parameters controls.
use crate::support_fetch;

#[test]
fn should_preserve_first_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST $1 ROWS ONLY";
    let text = "2".as_bytes();
    let binary = &[0, 0, 0, 0, 0, 0, 0, 2];
    // Act
    let batches = support_fetch::parameters("fetch-first", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 20);
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
fn should_preserve_sum_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT ($1::INT+1) ROW ONLY";
    let text = "1".as_bytes();
    let binary = &[0, 0, 0, 1];
    // Act
    let batches = support_fetch::parameters("fetch-sum", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 23);
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
fn should_preserve_offset_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET $1 ROWS FETCH FIRST ROW ONLY";
    let text = "1".as_bytes();
    let binary = &[0, 0, 0, 0, 0, 0, 0, 1];
    // Act
    let batches = support_fetch::parameters("fetch-offset", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 20);
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_reverse_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST $1 ROWS ONLY OFFSET 1 ROW";
    let text = "1".as_bytes();
    let binary = &[0, 0, 0, 0, 0, 0, 0, 1];
    // Act
    let batches = support_fetch::parameters("fetch-reverse", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 20);
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_negative_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-($1::INT)) ROWS ONLY";
    let text = "-2".as_bytes();
    let binary = &[255, 255, 255, 254];
    // Act
    let batches = support_fetch::parameters("fetch-negative", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 23);
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
fn should_preserve_int2_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ($1::SMALLINT) ROWS ONLY";
    let text = "2".as_bytes();
    let binary = &[0, 2];
    // Act
    let batches = support_fetch::parameters("fetch-int2", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 21);
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
fn should_preserve_int8_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ($1::BIGINT) ROWS ONLY";
    let text = "2".as_bytes();
    let binary = &[0, 0, 0, 0, 0, 0, 0, 2];
    // Act
    let batches = support_fetch::parameters("fetch-int8", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 20);
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
fn should_preserve_nested_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET ($1+1) ROWS FETCH NEXT ROW ONLY";
    let text = "0".as_bytes();
    let binary = &[0, 0, 0, 0];
    // Act
    let batches = support_fetch::parameters("fetch-nested", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 23);
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_infer_concrete_int4_given_a_concrete_sibling_when_fetch_parameters_are_bound() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ($1+1) ROWS ONLY";
    let text = "1".as_bytes();
    let binary = &[0, 0, 0, 1];
    // Act
    let batches = support_fetch::parameters("fetch-review-concrete_int4", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 23);
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
fn should_infer_concrete_int8_given_a_concrete_sibling_when_fetch_parameters_are_bound() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (9223372036854775807+$1) ROWS ONLY";
    let text = "-9223372036854775806".as_bytes();
    let binary = &[128, 0, 0, 0, 0, 0, 0, 2];
    // Act
    let batches = support_fetch::parameters("fetch-review-concrete_int8", sql, text, binary);
    // Assert
    support_fetch::assert_parameter(&batches, 20);
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}
