//! Unary operators follow native precedence, widths and wire result identities.
use crate::support_bind_planning as planning;
use crate::support_unary_numeric::assert_result;

#[test]
fn should_apply_int2_minus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(7::SMALLINT)";
    // Act
    let batches = planning::pruned("unary-int2_minus", sql);
    // Assert
    assert_result(
        &batches,
        21,
        [&[0, 1, 0, 0, 0, 2, 45, 55], &[0, 1, 0, 0, 0, 2, 255, 249]],
    );
}

#[test]
fn should_apply_int4_minus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(7::INT)";
    // Act
    let batches = planning::pruned("unary-int4_minus", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_int8_minus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(7::BIGINT)";
    // Act
    let batches = planning::pruned("unary-int8_minus", sql);
    // Assert
    assert_result(
        &batches,
        20,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_float8_minus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(7::FLOAT)";
    // Act
    let batches = planning::pruned("unary-float8_minus", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 8, 192, 28, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_int2_plus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +(7::SMALLINT)";
    // Act
    let batches = planning::pruned("unary-int2_plus", sql);
    // Assert
    assert_result(
        &batches,
        21,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 2, 0, 7]],
    );
}

#[test]
fn should_apply_int4_plus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +(7::INT)";
    // Act
    let batches = planning::pruned("unary-int4_plus", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
    );
}

#[test]
fn should_apply_int8_plus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +(7::BIGINT)";
    // Act
    let batches = planning::pruned("unary-int8_plus", sql);
    // Assert
    assert_result(
        &batches,
        20,
        [
            &[0, 1, 0, 0, 0, 1, 55],
            &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 7],
        ],
    );
}

#[test]
fn should_apply_float8_plus_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +(7::FLOAT)";
    // Act
    let batches = planning::pruned("unary-float8_plus", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 1, 55],
            &[0, 1, 0, 0, 0, 8, 64, 28, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_sum_operand_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(2::INT+3::INT)";
    // Act
    let batches = planning::pruned("unary-sum_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 53],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 251],
        ],
    );
}

#[test]
fn should_apply_product_operand_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(2::INT*3::INT)";
    // Act
    let batches = planning::pruned("unary-product_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 54],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 250],
        ],
    );
}

#[test]
fn should_apply_multiply_precedence_given_unary_syntax_when_native_numeric_rules_select_the_result()
{
    // Arrange
    let sql = "SELECT -2::INT*3::INT+7::INT";
    // Act
    let batches = planning::pruned("unary-multiply_precedence", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
    );
}

#[test]
fn should_apply_divide_signed_operand_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT 12::INT / -(3::INT)";
    // Act
    let batches = planning::pruned("unary-divide_signed_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 52],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 252],
        ],
    );
}

#[test]
fn should_apply_subtract_signed_operand_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT 7::INT - -(3::INT)";
    // Act
    let batches = planning::pruned("unary-subtract_signed_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 49, 48],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 10],
        ],
    );
}

#[test]
fn should_apply_nested_signs_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT - + -(7::INT)";
    // Act
    let batches = planning::pruned("unary-nested_signs", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
    );
}

#[test]
fn should_apply_plus_signed_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT + -(7::INT)";
    // Act
    let batches = planning::pruned("unary-plus_signed", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_case_operand_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(CASE WHEN TRUE THEN 7::INT ELSE 9::INT END)";
    // Act
    let batches = planning::pruned("unary-case_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_function_operand_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(abs(-7::INT))";
    // Act
    let batches = planning::pruned("unary-function_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_typed_int_null_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(NULL::INT)";
    // Act
    let batches = planning::pruned("unary-typed_int_null", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [&[0, 1, 255, 255, 255, 255], &[0, 1, 255, 255, 255, 255]],
    );
}

#[test]
fn should_apply_typed_float_null_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +(NULL::FLOAT)";
    // Act
    let batches = planning::pruned("unary-typed_float_null", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [&[0, 1, 255, 255, 255, 255], &[0, 1, 255, 255, 255, 255]],
    );
}

#[test]
fn should_apply_negative_zero_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(0::FLOAT)";
    // Act
    let batches = planning::pruned("unary-negative_zero", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 2, 45, 48],
            &[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_positive_negative_zero_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT +('-0'::FLOAT)";
    // Act
    let batches = planning::pruned("unary-positive_negative_zero", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 2, 45, 48],
            &[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_double_negative_zero_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT -('-0'::FLOAT)";
    // Act
    let batches = planning::pruned("unary-double_negative_zero", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 1, 48],
            &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_positive_infinity_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -('-Infinity'::FLOAT)";
    // Act
    let batches = planning::pruned("unary-positive_infinity", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 8, 73, 110, 102, 105, 110, 105, 116, 121],
            &[0, 1, 0, 0, 0, 8, 127, 240, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_negative_infinity_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -('Infinity'::FLOAT)";
    // Act
    let batches = planning::pruned("unary-negative_infinity", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 9, 45, 73, 110, 102, 105, 110, 105, 116, 121],
            &[0, 1, 0, 0, 0, 8, 255, 240, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_cast_after_parentheses_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT (-(7::BIGINT))::INT";
    // Act
    let batches = planning::pruned("unary-cast_after_parentheses", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_cast_before_negation_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT -(7::SMALLINT)::BIGINT";
    // Act
    let batches = planning::pruned("unary-cast_before_negation", sql);
    // Assert
    assert_result(
        &batches,
        20,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_minimum_literal_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT - 2147483648";
    // Act
    let batches = planning::pruned("unary-minimum_literal", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[
                0, 1, 0, 0, 0, 11, 45, 50, 49, 52, 55, 52, 56, 51, 54, 52, 56,
            ],
            &[0, 1, 0, 0, 0, 4, 128, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_minimum_int8_literal_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT - 9223372036854775808";
    // Act
    let batches = planning::pruned("unary-minimum_int8_literal", sql);
    // Assert
    assert_result(
        &batches,
        20,
        [
            &[
                0, 1, 0, 0, 0, 20, 45, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55,
                53, 56, 48, 56,
            ],
            &[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_values_int_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "VALUES (-(7::INT))";
    // Act
    let batches = planning::pruned("unary-values_int", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_values_float_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "VALUES (-(7::FLOAT))";
    // Act
    let batches = planning::pruned("unary-values_float", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 8, 192, 28, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_coalesce_operand_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT -(COALESCE(NULL::INT,7::INT))";
    // Act
    let batches = planning::pruned("unary-coalesce_operand", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_boolean_comparison_given_unary_syntax_when_native_numeric_rules_select_the_result()
{
    // Arrange
    let sql = "SELECT -(7::INT) = -7::INT";
    // Act
    let batches = planning::pruned("unary-boolean_comparison", sql);
    // Assert
    assert_result(
        &batches,
        16,
        [&[0, 1, 0, 0, 0, 1, 116], &[0, 1, 0, 0, 0, 1, 1]],
    );
}

#[test]
fn should_apply_pruned_dead_arm_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT CASE WHEN 7::INT=8::INT THEN -((-2147483648)::INT) ELSE 7::INT END";
    // Act
    let batches = planning::pruned("unary-pruned_dead_arm", sql);
    // Assert
    assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
    );
}

#[test]
fn should_apply_int2_overflow_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("SELECT -((-32768)::SMALLINT)", "22003")];
    // Act
    let batches = planning::failures("unary-int2_overflow", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_int4_overflow_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("SELECT -((-2147483648)::INT)", "22003")];
    // Act
    let batches = planning::failures("unary-int4_overflow", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_int8_overflow_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("SELECT -((-9223372036854775808)::BIGINT)", "22003")];
    // Act
    let batches = planning::failures("unary-int8_overflow", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_cast_precedence_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("SELECT -32768::SMALLINT", "22003")];
    // Act
    let batches = planning::failures("unary-cast_precedence", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_parenthesized_minimum_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let cases = [("SELECT -(-9223372036854775808::BIGINT)", "22003")];
    // Act
    let batches = planning::failures("unary-parenthesized_minimum", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_division_operand_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("SELECT -(1/0)", "22012")];
    // Act
    let batches = planning::failures("unary-division_operand", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_dead_dynamic_arm_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("SELECT CASE WHEN n=8 THEN -((-2147483648)::INT) ELSE 7::INT END FROM (VALUES (7),(9)) AS v(n)", "22003")];
    // Act
    let batches = planning::failures("unary-dead_dynamic_arm", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_values_overflow_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let cases = [("VALUES (-((-2147483648)::INT))", "22003")];
    // Act
    let batches = planning::failures("unary-values_overflow", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_widen_int2_before_minus_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT -((-32768)::SMALLINT)::BIGINT";
    // Act
    let batches = planning::pruned("unary-widen_int2_before_minus", sql);
    // Assert
    assert_result(
        &batches,
        20,
        [
            &[0, 1, 0, 0, 0, 5, 51, 50, 55, 54, 56],
            &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 128, 0],
        ],
    );
}

#[test]
fn should_apply_widen_int4_before_minus_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let sql = "SELECT -((-2147483648)::INT)::BIGINT";
    // Act
    let batches = planning::pruned("unary-widen_int4_before_minus", sql);
    // Assert
    assert_result(
        &batches,
        20,
        [
            &[0, 1, 0, 0, 0, 10, 50, 49, 52, 55, 52, 56, 51, 54, 52, 56],
            &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 128, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_widen_int2_after_minus_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let cases = [("SELECT (-((-32768)::SMALLINT))::BIGINT", "22003")];
    // Act
    let batches = planning::failures("unary-widen_int2_after_minus", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_apply_widen_int4_after_minus_given_unary_syntax_when_native_numeric_rules_select_the_result(
) {
    // Arrange
    let cases = [("SELECT (-((-2147483648)::INT))::BIGINT", "22003")];
    // Act
    let batches = planning::failures("unary-widen_int4_after_minus", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_unknown_minus_null_given_parse_flush_when_unary_signatures_require_resolution() {
    // Arrange
    let sql = "SELECT -NULL";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-unknown_minus_null", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_unknown_minus_text_given_parse_flush_when_unary_signatures_require_resolution() {
    // Arrange
    let sql = "SELECT -'7'";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-unknown_minus_text", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_unknown_minus_bad_given_parse_flush_when_unary_signatures_require_resolution() {
    // Arrange
    let sql = "SELECT -'bad'";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-unknown_minus_bad", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_known_minus_boolean_given_parse_flush_when_unary_signatures_require_resolution() {
    // Arrange
    let sql = "SELECT -(TRUE)";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-known_minus_boolean", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_known_plus_text_given_parse_flush_when_unary_signatures_require_resolution() {
    // Arrange
    let sql = "SELECT +(CAST('7' AS TEXT))";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-known_plus_text", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_unknown_plus_bad_given_parse_flush_when_unary_signatures_require_resolution() {
    // Arrange
    let sql = "SELECT +'bad'";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-unknown_plus_bad", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "22P02");
}

#[test]
fn should_apply_unknown_plus_null_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +NULL";
    // Act
    let batches = planning::pruned("unary-unknown_plus_null", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [&[0, 1, 255, 255, 255, 255], &[0, 1, 255, 255, 255, 255]],
    );
}

#[test]
fn should_apply_unknown_plus_text_given_unary_syntax_when_native_numeric_rules_select_the_result() {
    // Arrange
    let sql = "SELECT +'7'";
    // Act
    let batches = planning::pruned("unary-unknown_plus_text", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 1, 55],
            &[0, 1, 0, 0, 0, 8, 64, 28, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_apply_int2_parameter_given_cast_signatures_when_parameters_are_described_then_executed() {
    // Arrange
    let sql = "SELECT -($1::SMALLINT)";
    // Act
    let batches = crate::support_unary_numeric::parameters("unary-int2_parameter", sql, b"7");
    // Assert
    crate::support_unary_numeric::assert_parameter(&batches, 21);
    assert_result(
        &batches,
        21,
        [&[0, 1, 0, 0, 0, 2, 45, 55], &[0, 1, 0, 0, 0, 2, 255, 249]],
    );
}

#[test]
fn should_apply_int4_parameter_given_cast_signatures_when_parameters_are_described_then_executed() {
    // Arrange
    let sql = "SELECT -($1::INT)";
    // Act
    let batches = crate::support_unary_numeric::parameters("unary-int4_parameter", sql, b"7");
    // Assert
    crate::support_unary_numeric::assert_parameter(&batches, 23);
    assert_result(
        &batches,
        23,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 4, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_int8_parameter_given_cast_signatures_when_parameters_are_described_then_executed() {
    // Arrange
    let sql = "SELECT -($1::BIGINT)";
    // Act
    let batches = crate::support_unary_numeric::parameters("unary-int8_parameter", sql, b"7");
    // Assert
    crate::support_unary_numeric::assert_parameter(&batches, 20);
    assert_result(
        &batches,
        20,
        [
            &[0, 1, 0, 0, 0, 2, 45, 55],
            &[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 249],
        ],
    );
}

#[test]
fn should_apply_float8_parameter_given_cast_signatures_when_parameters_are_described_then_executed()
{
    // Arrange
    let sql = "SELECT -($1::FLOAT)";
    // Act
    let batches = crate::support_unary_numeric::parameters("unary-float8_parameter", sql, b"0");
    // Assert
    crate::support_unary_numeric::assert_parameter(&batches, 701);
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 2, 45, 48],
            &[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_resolve_unary_unknown_context_predicate_given_enclosing_expressions_when_native_types_are_selected(
) {
    // Arrange
    let sql = "SELECT 7::INT WHERE +NULL";
    // Act
    let batches = crate::support_unary_numeric::parse("unary-context-0", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42804");
}

#[test]
fn should_resolve_unary_unknown_context_coalesce_given_enclosing_expressions_when_native_types_are_selected(
) {
    // Arrange
    let sql = "SELECT COALESCE(+NULL,7::INT)";
    // Act
    let batches = planning::pruned("unary-context-1", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [
            &[0, 1, 0, 0, 0, 1, 55],
            &[0, 1, 0, 0, 0, 8, 64, 28, 0, 0, 0, 0, 0, 0],
        ],
    );
}

#[test]
fn should_resolve_unary_unknown_context_case_result_given_enclosing_expressions_when_native_types_are_selected(
) {
    // Arrange
    let sql = "SELECT CASE WHEN TRUE THEN +NULL ELSE 7::INT END";
    // Act
    let batches = planning::pruned("unary-context-2", sql);
    // Assert
    assert_result(
        &batches,
        701,
        [&[0, 1, 255, 255, 255, 255], &[0, 1, 255, 255, 255, 255]],
    );
}

#[test]
fn should_resolve_unary_unknown_context_case_operand_given_enclosing_expressions_when_native_types_are_selected(
) {
    // Arrange
    let sql = "SELECT CASE +'7' WHEN $1 THEN 7::INT ELSE 9::INT END";
    // Act
    let batches = crate::support_unary_numeric::parameters("unary-context-3", sql, b"7");
    // Assert
    crate::support_unary_numeric::assert_parameter(&batches, 701);
    assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
    );
}
