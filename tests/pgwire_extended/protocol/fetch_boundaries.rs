//! Adversarial count grammar, clause ordering and lexical boundaries.
use crate::support_bind_planning as planning;
use crate::support_fetch;

#[test]
fn should_reject_double_plus_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ++1 ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-double_plus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_double_minus_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST - -1 ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-double_minus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_signed_parenthesis_plus_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST +(1) ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-signed_parenthesis_plus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_signed_parenthesis_minus_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST -(1) ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-signed_parenthesis_minus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_offset_signed_parenthesis_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET +(1) ROWS FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-offset_signed_parenthesis", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_offset_double_sign_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET ++1 ROW FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-offset_double_sign", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_late_order_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT 7::INT FETCH FIRST ROW ONLY ORDER BY 1";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-late_order", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_late_where_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT 7::INT FETCH FIRST ROW ONLY WHERE TRUE";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-late_where", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_late_group_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT 7::INT FETCH FIRST ROW ONLY GROUP BY 1";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-late_group", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_reject_late_offset_order_given_standard_counts_when_native_grammar_requires_parentheses_or_order(
) {
    // Arrange
    let sql = "SELECT 7::INT OFFSET 0 ROWS ORDER BY 1 FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-probe-late_offset_order", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_comments_given_lexical_boundaries_when_fetch_is_lowered() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH/*outer/*inner*/tail*/FIRST/*count*/(1+1)/*marker*/ROWS/*end*/ONLY";
    // Act
    let batches = planning::pruned("fetch-probe-comments", sql);
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
fn should_preserve_signed_comment_given_lexical_boundaries_when_fetch_is_lowered() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT +/*signed*/1 ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-probe-signed_comment", sql);
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
fn should_preserve_quoted_alias_given_lexical_boundaries_when_fetch_is_lowered() {
    // Arrange
    let sql = "SELECT 7::INT AS \"fetch\" FETCH FIRST ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-probe-quoted_alias", sql);
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
fn should_preserve_quoted_count_alias_given_lexical_boundaries_when_fetch_is_lowered() {
    // Arrange
    let sql = "SELECT * FROM (VALUES (7::INT),(8::INT)) AS \"rows\"(\"only\") ORDER BY \"only\" FETCH NEXT ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-probe-quoted_count_alias", sql);
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
fn should_preserve_quoted_marker_given_lexical_boundaries_when_fetch_is_lowered() {
    // Arrange
    let sql = "SELECT 7::INT AS \"ROW FETCH OFFSET\" OFFSET 0 ROWS FETCH FIRST ROW ONLY";
    // Act
    let batches = planning::pruned("fetch-probe-quoted_marker", sql);
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
fn should_preserve_reverse_comments_given_lexical_boundaries_when_fetch_is_lowered() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT ROW ONLY OFFSET/*x*/1/*y*/ROWS";
    // Act
    let batches = planning::pruned("fetch-probe-reverse_comments", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}
