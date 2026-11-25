//! Prefix signature selection precedes integer pagination admission.
use crate::support_bind_planning as planning;
use crate::support_pagination_unary::assert_rows;
use crate::support_unary_numeric as unary;

#[test]
fn should_preserve_unknown_null_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -NULL";
    // Act
    let batches = unary::parse("page-unknown_null_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_unknown_null_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -NULL";
    // Act
    let batches = unary::parse("page-unknown_null_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_unknown_text_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -'1'";
    // Act
    let batches = unary::parse("page-unknown_text_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_unknown_text_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -'1'";
    // Act
    let batches = unary::parse("page-unknown_text_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_unknown_bad_text_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -'bad'";
    // Act
    let batches = unary::parse("page-unknown_bad_text_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_unknown_bad_text_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -'bad'";
    // Act
    let batches = unary::parse("page-unknown_bad_text_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_outer_int_unknown_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT (-NULL)::INT";
    // Act
    let batches = unary::parse("page-outer_int_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_outer_int_unknown_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET (-NULL)::INT";
    // Act
    let batches = unary::parse("page-outer_int_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_outer_bigint_unknown_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT (-'1')::BIGINT";
    // Act
    let batches = unary::parse("page-outer_bigint_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_outer_bigint_unknown_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET (-'1')::BIGINT";
    // Act
    let batches = unary::parse("page-outer_bigint_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_sum_unknown_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT (-NULL)+1";
    // Act
    let batches = unary::parse("page-sum_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_sum_unknown_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET (-NULL)+1";
    // Act
    let batches = unary::parse("page-sum_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_nested_unknown_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(-NULL)";
    // Act
    let batches = unary::parse("page-nested_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_nested_unknown_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(-NULL)";
    // Act
    let batches = unary::parse("page-nested_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_plus_unknown_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(-NULL)";
    // Act
    let batches = unary::parse("page-plus_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_plus_unknown_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(-NULL)";
    // Act
    let batches = unary::parse("page-plus_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_preserve_bool_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +TRUE";
    // Act
    let batches = unary::parse("page-bool_plus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_bool_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +TRUE";
    // Act
    let batches = unary::parse("page-bool_plus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_bool_minus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -FALSE";
    // Act
    let batches = unary::parse("page-bool_minus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_bool_minus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -FALSE";
    // Act
    let batches = unary::parse("page-bool_minus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_bool_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(TRUE::BOOLEAN)";
    // Act
    let batches = unary::parse("page-typed_bool_plus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_bool_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(TRUE::BOOLEAN)";
    // Act
    let batches = unary::parse("page-typed_bool_plus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_bool_minus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(NULL::BOOLEAN)";
    // Act
    let batches = unary::parse("page-typed_bool_minus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_bool_minus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(NULL::BOOLEAN)";
    // Act
    let batches = unary::parse("page-typed_bool_minus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_text_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +('1'::TEXT)";
    // Act
    let batches = unary::parse("page-typed_text_plus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_text_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +('1'::TEXT)";
    // Act
    let batches = unary::parse("page-typed_text_plus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_text_minus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -('1'::TEXT)";
    // Act
    let batches = unary::parse("page-typed_text_minus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_text_minus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -('1'::TEXT)";
    // Act
    let batches = unary::parse("page-typed_text_minus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_varchar_minus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -('1'::VARCHAR)";
    // Act
    let batches = unary::parse("page-typed_varchar_minus_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_typed_varchar_minus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -('1'::VARCHAR)";
    // Act
    let batches = unary::parse("page-typed_varchar_minus_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_nested_bool_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(+TRUE)";
    // Act
    let batches = unary::parse("page-nested_bool_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_nested_bool_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(+TRUE)";
    // Act
    let batches = unary::parse("page-nested_bool_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_preserve_null_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +NULL";
    // Act
    let batches = planning::pruned("page-null_plus_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_null_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +NULL";
    // Act
    let batches = planning::pruned("page-null_plus_offset", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int2_null_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(NULL::SMALLINT)";
    // Act
    let batches = planning::pruned("page-int2_null_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int2_null_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(NULL::SMALLINT)";
    // Act
    let batches = planning::pruned("page-int2_null_offset", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int4_null_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(NULL::INT)";
    // Act
    let batches = planning::pruned("page-int4_null_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int4_null_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(NULL::INT)";
    // Act
    let batches = planning::pruned("page-int4_null_offset", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int8_null_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(NULL::BIGINT)";
    // Act
    let batches = planning::pruned("page-int8_null_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int8_null_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(NULL::BIGINT)";
    // Act
    let batches = planning::pruned("page-int8_null_offset", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int2_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(1::SMALLINT)";
    // Act
    let batches = planning::pruned("page-int2_plus_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int2_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(1::SMALLINT)";
    // Act
    let batches = planning::pruned("page-int2_plus_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_int4_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(1::INT)";
    // Act
    let batches = planning::pruned("page-int4_plus_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int4_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(1::INT)";
    // Act
    let batches = planning::pruned("page-int4_plus_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_int8_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(1::BIGINT)";
    // Act
    let batches = planning::pruned("page-int8_plus_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int8_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(1::BIGINT)";
    // Act
    let batches = planning::pruned("page-int8_plus_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_int2_twice_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(-(1::SMALLINT))";
    // Act
    let batches = planning::pruned("page-int2_twice_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int2_twice_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(-(1::SMALLINT))";
    // Act
    let batches = planning::pruned("page-int2_twice_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_int4_twice_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(-(1::INT))";
    // Act
    let batches = planning::pruned("page-int4_twice_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int4_twice_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(-(1::INT))";
    // Act
    let batches = planning::pruned("page-int4_twice_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_int8_twice_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(-(1::BIGINT))";
    // Act
    let batches = planning::pruned("page-int8_twice_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_int8_twice_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(-(1::BIGINT))";
    // Act
    let batches = planning::pruned("page-int8_twice_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_zero_minus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(0::INT)";
    // Act
    let batches = planning::pruned("page-zero_minus_limit", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_zero_minus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(0::INT)";
    // Act
    let batches = planning::pruned("page-zero_minus_offset", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_zero_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(0::SMALLINT)";
    // Act
    let batches = planning::pruned("page-zero_plus_limit", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_zero_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(0::SMALLINT)";
    // Act
    let batches = planning::pruned("page-zero_plus_offset", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_sum_minus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(0::INT-1::INT)";
    // Act
    let batches = planning::pruned("page-sum_minus_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_sum_minus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(0::INT-1::INT)";
    // Act
    let batches = planning::pruned("page-sum_minus_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_cast_widen_before_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -((-32768)::SMALLINT)::BIGINT";
    // Act
    let batches = planning::pruned("page-cast_widen_before_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_cast_widen_before_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -((-32768)::SMALLINT)::BIGINT";
    // Act
    let batches = planning::pruned("page-cast_widen_before_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_sum_plus_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT +(1::INT+1::INT)";
    // Act
    let batches = planning::pruned("page-sum_plus_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_sum_plus_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET +(1::INT+1::INT)";
    // Act
    let batches = planning::pruned("page-sum_plus_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_preserve_mixed_prefix_limit_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT LIMIT - (+(-(1::INT)))";
    // Act
    let batches = planning::pruned("page-mixed_prefix_limit", sql);
    // Assert
    assert_rows(&batches, 1);
}

#[test]
fn should_preserve_mixed_prefix_offset_given_prefix_syntax_when_pagination_is_analyzed() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET - (+(-(1::INT)))";
    // Act
    let batches = planning::pruned("page-mixed_prefix_offset", sql);
    // Assert
    assert_rows(&batches, 0);
}

#[test]
fn should_reject_cte_unknown_limit_given_nested_pagination_when_parse_checks_operator_signatures() {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT LIMIT -NULL) SELECT * FROM x";
    // Act
    let batches = unary::parse("page-cte_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_cte_unknown_offset_given_nested_pagination_when_parse_checks_operator_signatures()
{
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT OFFSET -NULL) SELECT * FROM x";
    // Act
    let batches = unary::parse("page-cte_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_cte_boolean_limit_given_nested_pagination_when_parse_checks_operator_signatures() {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT LIMIT +TRUE) SELECT * FROM x";
    // Act
    let batches = unary::parse("page-cte_boolean_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_cte_boolean_offset_given_nested_pagination_when_parse_checks_operator_signatures()
{
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT OFFSET +TRUE) SELECT * FROM x";
    // Act
    let batches = unary::parse("page-cte_boolean_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_unused_cte_unknown_limit_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT LIMIT -NULL) SELECT 7::INT";
    // Act
    let batches = unary::parse("page-unused_cte_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_unused_cte_unknown_offset_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT OFFSET -NULL) SELECT 7::INT";
    // Act
    let batches = unary::parse("page-unused_cte_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_unused_cte_boolean_limit_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT LIMIT +TRUE) SELECT 7::INT";
    // Act
    let batches = unary::parse("page-unused_cte_boolean_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_unused_cte_boolean_offset_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT OFFSET +TRUE) SELECT 7::INT";
    // Act
    let batches = unary::parse("page-unused_cte_boolean_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_derived_unknown_limit_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT LIMIT -NULL) AS x";
    // Act
    let batches = unary::parse("page-derived_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_derived_unknown_offset_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT OFFSET -NULL) AS x";
    // Act
    let batches = unary::parse("page-derived_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_derived_boolean_limit_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT LIMIT +TRUE) AS x";
    // Act
    let batches = unary::parse("page-derived_boolean_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_derived_boolean_offset_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT * FROM (SELECT 7::INT OFFSET +TRUE) AS x";
    // Act
    let batches = unary::parse("page-derived_boolean_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_exists_unknown_limit_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT EXISTS (SELECT 7::INT LIMIT -NULL)";
    // Act
    let batches = unary::parse("page-exists_unknown_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_exists_unknown_offset_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT EXISTS (SELECT 7::INT OFFSET -NULL)";
    // Act
    let batches = unary::parse("page-exists_unknown_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42725");
}

#[test]
fn should_reject_exists_boolean_limit_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT EXISTS (SELECT 7::INT LIMIT +TRUE)";
    // Act
    let batches = unary::parse("page-exists_boolean_limit", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_exists_boolean_offset_given_nested_pagination_when_parse_checks_operator_signatures(
) {
    // Arrange
    let sql = "SELECT EXISTS (SELECT 7::INT OFFSET +TRUE)";
    // Act
    let batches = unary::parse("page-exists_boolean_offset", sql);
    // Assert
    unary::assert_parse_failure(&batches, "42883");
}

#[test]
fn should_reject_int2_overflow_limit_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [("SELECT 7::INT LIMIT -((-32768)::SMALLINT)", "22003")];
    // Act
    let batches = planning::failures("page-int2_overflow_limit", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_int2_overflow_offset_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [("SELECT 7::INT OFFSET -((-32768)::SMALLINT)", "22003")];
    // Act
    let batches = planning::failures("page-int2_overflow_offset", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_int4_overflow_limit_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [("SELECT 7::INT LIMIT -((-2147483648)::INT)", "22003")];
    // Act
    let batches = planning::failures("page-int4_overflow_limit", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_int4_overflow_offset_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [("SELECT 7::INT OFFSET -((-2147483648)::INT)", "22003")];
    // Act
    let batches = planning::failures("page-int4_overflow_offset", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_int8_overflow_limit_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [(
        "SELECT 7::INT LIMIT -((-9223372036854775808)::BIGINT)",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-int8_overflow_limit", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_int8_overflow_offset_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [(
        "SELECT 7::INT OFFSET -((-9223372036854775808)::BIGINT)",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-int8_overflow_offset", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_widen_after_limit_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [(
        "SELECT 7::INT LIMIT (-((-32768)::SMALLINT))::BIGINT",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-widen_after_limit", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_reject_widen_after_offset_given_checked_unary_width_when_bind_plans_pagination() {
    // Arrange
    let cases = [(
        "SELECT 7::INT OFFSET (-((-32768)::SMALLINT))::BIGINT",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-widen_after_offset", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}
