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
fn should_bind_parenthesized_cast_result_through_postfix_conversion() {
    // Arrange
    let fixture = sql_fixture("issue-796-parenthesized-postfix-cast", &[]);

    // Act
    let postfix = fixture.execute("SELECT (CAST('42' AS TEXT))::INT AS value");
    let standard = fixture.execute("SELECT CAST(CAST('42' AS TEXT) AS INT) AS value");

    // Assert
    let postfix = postfix.expect("postfix cast after parenthesized CAST");
    let standard = standard.expect("nested CAST control");
    assert_eq!(postfix.columns()[0].type_oid, 23);
    assert_eq!(standard.columns()[0].type_oid, 23);
    assert_eq!(postfix.rows(), standard.rows());
    assert_eq!(postfix.rows(), &[vec![Value::Int64(42)]]);
}

#[test]
fn should_reject_legacy_type_aliases_in_sql_type_positions_given_array_suffixes() {
    // Arrange
    let fixture = sql_fixture("issue-796-legacy-type-aliases", &[]);
    let cases = [
        (
            "CREATE TABLE issue_796_double_array (value DOUBLE[])",
            "double",
        ),
        ("SELECT CAST('word' AS STRING) AS value", "string"),
        ("SELECT 'word'::STRING AS value", "string"),
        (
            "CREATE TABLE issue_796_string_array (value STRING[])",
            "string",
        ),
        (
            "CREATE TABLE issue_796_string_column (value STRING)",
            "string",
        ),
    ];

    // Act
    let outcomes = cases
        .iter()
        .map(|(sql, alias)| (sql, alias, fixture.execute(sql)))
        .collect::<Vec<_>>();

    // Assert
    let failures = outcomes
        .into_iter()
        .filter_map(|(sql, alias, result)| match result {
            Err(cassie::app::CassieError::Unsupported(message))
                if message == format!("unsupported data type '{alias}'") => None,
            Ok(result) => Some(format!("{sql}: unexpectedly accepted {result:?}")),
            Err(error) => Some(format!("{sql}: expected unsupported alias rejection, got {error}")),
        })
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "legacy aliases must not enter through SQL type positions:\n{}",
        failures.join("\n")
    );
    assert_eq!(
        cassie::types::DataType::parse_sql("double[]"),
        Ok(cassie::types::DataType::Array(Box::new(
            cassie::types::DataType::Float
        ))),
        "non-SQL helper callers retain the array alias"
    );
    assert_eq!(
        cassie::types::DataType::parse_sql("string"),
        Ok(cassie::types::DataType::Text),
        "non-SQL helper callers retain the text alias"
    );
}

#[test]
fn should_reject_bare_double_given_postgresql_type_name_rules_when_casting_and_declaring() {
    // Arrange
    let fixture = sql_fixture("issue-796-bare-double-type-name", &[]);
    let statements = [
        "SELECT CAST('1.5' AS DOUBLE) AS value",
        "SELECT '1.5'::DOUBLE AS value",
        "CREATE TABLE issue_796_bare_double_rows (value DOUBLE)",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert!(
            matches!(
                &result,
                Err(cassie::app::CassieError::Unsupported(message))
                    if message == "unsupported data type 'double'"
            ),
            "{sql}: expected deterministic unsupported-type rejection, got {result:?}"
        );
    }
    assert_eq!(
        cassie::types::DataType::parse_sql("double"),
        Ok(cassie::types::DataType::Float),
        "non-SQL DataType callers retain the existing alias"
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
