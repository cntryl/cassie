//! Bound names use source scope before rejecting row-dependent pagination.

#[test]
fn should_reject_unknown_row_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT OFFSET ROW FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-unknown_row", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42703");
}

#[test]
fn should_reject_unknown_fetch_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT FETCH NEXT missing ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-unknown_fetch", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42703");
}

#[test]
fn should_reject_source_column_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT n FROM (VALUES (7::INT)) AS v(n) FETCH FIRST n ROWS ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-source_column", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42P10");
}

#[test]
fn should_reject_qualified_column_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT n FROM (VALUES (7::INT)) AS v(n) OFFSET v.n ROWS FETCH FIRST ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-qualified_column", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42P10");
}

#[test]
fn should_reject_unknown_qualified_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT n FROM (VALUES (7::INT)) AS v(n) FETCH NEXT v.missing ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-unknown_qualified", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42703");
}

#[test]
fn should_reject_output_alias_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT AS bound FETCH FIRST bound ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-output_alias", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42703");
}

#[test]
fn should_reject_ambiguous_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT 7::INT FROM (VALUES (1)) AS a(n) JOIN (VALUES (2)) AS b(n) ON TRUE FETCH NEXT n ROW ONLY";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-ambiguous", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42702");
}

#[test]
fn should_reject_quoted_row_given_pagination_counts_when_native_source_name_rules_apply() {
    // Arrange
    let sql = "SELECT \"row\" FROM (VALUES (7::INT)) AS v(\"row\") OFFSET \"row\" ROWS";
    // Act
    let batches = crate::support_unary_numeric::parse("fetch-name-quoted_row", sql);
    // Assert
    crate::support_unary_numeric::assert_parse_failure(&batches, "42P10");
}
