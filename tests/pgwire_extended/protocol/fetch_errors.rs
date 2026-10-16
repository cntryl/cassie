//! PostgreSQL FETCH/OFFSET errors controls.
use crate::support_bind_planning as planning;

#[test]
fn should_preserve_syntax_0_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_0", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_1_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_1", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_2_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ROW";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_2", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_3_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROWS";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_3", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_4_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ALL ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_4", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_5_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1+1 ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_5", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_6_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1::INT ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_6", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_7_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST + (1::INT) ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_7", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_8_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST -NULL ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_8", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_9_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST +NULL ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_9", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_10_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROW ONLY ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_10", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_11_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROWS ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_11", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_12_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH LAST 1 ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_12", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_13_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT 1 ROWS BEFORE";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_13", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_14_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ROW ONLY FETCH NEXT ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_14", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_15_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n LIMIT 1 FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_15", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_16_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ROW ONLY LIMIT 1";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_16", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_17_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 1 ROW OFFSET 2 ROW";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_17", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_18_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 1+1 ROWS";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_18", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_19_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET 1::INT ROW";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_19", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_20_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n OFFSET ROW FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_20", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42703");
}

#[test]
fn should_preserve_syntax_21_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (1+1 ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_21", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_22_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROWS ONLY OFFSET 1 ROWS ROWS";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_22", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_syntax_23_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST 1 ROWS ONLY FIRST";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-syntax_23", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42601");
}

#[test]
fn should_preserve_unknown_null_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-NULL) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-unknown_null", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_unknown_string_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-'1') ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-unknown_string", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_true_plus_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (+TRUE) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-true_plus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_false_minus_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-FALSE) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-false_minus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_text_minus_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-('1'::TEXT)) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-text_minus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_bool_null_plus_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (+(NULL::BOOLEAN)) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-bool_null_plus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_varchar_plus_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (+('1'::VARCHAR)) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-varchar_plus", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_nested_unknown_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-(-NULL)) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-nested_unknown", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_int2_overflow_first_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-((-32768)::SMALLINT)) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-int2_overflow_first", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_int2_overflow_next_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT (-((-32768)::SMALLINT)) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-int2_overflow_next", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_int4_overflow_first_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-((-2147483648)::INT)) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-int4_overflow_first", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_int4_overflow_next_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT (-((-2147483648)::INT)) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-int4_overflow_next", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_int8_overflow_first_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (-((-9223372036854775808)::BIGINT)) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-int8_overflow_first", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_int8_overflow_next_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT (-((-9223372036854775808)::BIGINT)) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-int8_overflow_next", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_widen_after_first_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ((-((-32768)::SMALLINT))::BIGINT) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-widen_after_first", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_widen_after_next_given_fetch_syntax_when_native_pagination_rules_apply() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH NEXT ((-((-32768)::SMALLINT))::BIGINT) ROWS ONLY";
    let cases = [(sql, "22003")];
    // Act
    let batches = planning::failures("fetch-widen_after_next", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_unknown_pair_given_fetch_count_parameters_when_no_argument_signature_is_selected()
{
    // Arrange
    let sql =
        "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST ($1+$2) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-review-unknown_pair", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_outer_cast_unknown_given_fetch_count_parameters_when_no_argument_signature_is_selected(
) {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (7),(8),(9)) AS v(n) ORDER BY n FETCH FIRST (CAST($1+$2 AS BIGINT)) ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-review-outer_cast_unknown", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}
