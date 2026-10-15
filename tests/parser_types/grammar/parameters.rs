#[test]
fn should_bind_double_precision_type_names_in_casts() {
    // Arrange
    let fixture = sql_fixture("issue-796-double-precision", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('1.5' AS DOUBLE PRECISION) AS value");

    // Assert
    assert_eq!(
        result.expect("DOUBLE PRECISION cast").rows(),
        &[vec![Value::Float64(1.5)]]
    );
}

#[test]
fn should_accept_type_keyword_separators_given_double_precision_casts_when_projected() {
    // Arrange
    let fixture = sql_fixture("issue-796-double-precision-separators", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('1.5' AS DOUBLE\n\tPRECISION) AS value");

    // Assert
    let result = result.expect("whitespace separates the DOUBLE PRECISION type keywords");
    assert_eq!(result.rows(), &[vec![Value::Float64(1.5)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_bind_postgres_chained_type_casts() {
    // Arrange
    let fixture = sql_fixture("issue-796-chained-type-casts", &[]);

    // Act
    let result = fixture.execute("SELECT '7'::TEXT::INT4 AS chained");

    // Assert
    let result = result.expect("chained casts bind through the preceding target type");
    assert_eq!(result.rows(), &[vec![Value::Int64(7)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_evaluate_parenthesized_cast_operands_given_nested_arithmetic_when_projected() {
    // Arrange
    let fixture = sql_fixture("issue-796-parenthesized-cast-arithmetic", &[]);

    // Act
    let result = fixture.execute(
        "SELECT (1 + 2)::INT8 * (3 + 4)::INT8 AS product, \
         CAST((8 - 2) / (1 + 2) AS INT4)::INT8 AS quotient",
    );

    // Assert
    let result = result.expect("parenthesized operands retain their arithmetic inside each cast");
    assert_eq!(result.rows(), &[vec![Value::Int64(21), Value::Int64(2)]]);
    assert_eq!(result.columns()[0].type_oid, 20);
    assert_eq!(result.columns()[1].type_oid, 20);
}

#[test]
fn should_preserve_builtin_identity_given_pg_catalog_qualified_casts_when_projected() {
    // Arrange
    let fixture = sql_fixture("issue-796-qualified-builtin-casts", &[]);

    // Act
    let result = fixture.execute(
        "SELECT '7'::pg_catalog.int4 AS integer_value, \
         '1.5'::pg_catalog.float8 AS float_value, \
         'word'::pg_catalog.varchar(4) AS text_value",
    );

    // Assert
    let result = result.expect("qualified names resolve only registered builtin identities");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(7),
            Value::Float64(1.5),
            Value::String("word".to_owned()),
        ]]
    );
    assert_eq!(result.columns()[0].type_oid, 23);
    assert_eq!(result.columns()[1].type_oid, 701);
    assert_eq!(result.columns()[2].type_oid, 1043);
}

#[test]
fn should_preserve_case_arguments_given_inner_operators_and_postfix_casts_when_functions_are_evaluated(
) {
    // Arrange
    let fixture = sql_fixture("issue-796-case-function-arguments", &[]);

    // Act
    let result = fixture.execute(
        "SELECT coalesce(CASE WHEN 1 = 1 THEN 7 ELSE 0 END, 0) AS compared, \
         coalesce(CASE WHEN TRUE THEN 1 + 2 ELSE 0 END, 0) AS arithmetic, \
         coalesce(CASE WHEN TRUE THEN '7' ELSE '0' END::INT8, 0) AS casted",
    );

    // Assert
    let result = result.expect("CASE bodies stay atomic and allow an outer postfix cast");
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(7), Value::Int64(3), Value::Int64(7)]]
    );
    assert_eq!(result.columns()[0].type_oid, 23);
    assert_eq!(result.columns()[1].type_oid, 23);
    assert_eq!(result.columns()[2].type_oid, 20);
}

#[test]
fn should_preserve_dollar_quoted_case_results_given_keywords_and_apostrophes_when_projected_and_passed_to_functions(
) {
    // Arrange
    let fixture = sql_fixture("issue-796-case-dollar-literals", &[]);

    // Act
    let result = fixture.execute(
        "SELECT CASE WHEN TRUE THEN $$END$$ ELSE 'fallback' END AS keyword, \
         coalesce(CASE WHEN TRUE THEN $tag$it's WHEN /* END */$tag$ \
         ELSE 'fallback' END, 'outside') AS quoted",
    );

    // Assert
    let result = result.expect("literal CASE keywords and apostrophes do not end their CASE body");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("END".to_owned()),
            Value::String("it's WHEN /* END */".to_owned()),
        ]]
    );
    assert_eq!(result.columns()[0].type_oid, 25);
    assert_eq!(result.columns()[1].type_oid, 25);
}
