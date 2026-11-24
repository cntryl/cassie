#[test]
fn should_accept_postgres_float_type_aliases() {
    // Arrange
    let label = "issue-796-float-type-aliases";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        (
            "SELECT CAST('1.23456789' AS float4) AS value",
            vec![Value::String("1.2345679".to_owned())],
            700,
        ),
        (
            "SELECT CAST('1.23456789' AS float8) AS value",
            vec![Value::Float64(1.234_567_89)],
            701,
        ),
        (
            "SELECT CAST('1.23456789' AS real) AS value",
            vec![Value::String("1.2345679".to_owned())],
            700,
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_values, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [expected_values.clone()]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and values {expected_values:?}, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "float alias cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_evaluate_postgres_function_style_type_casts() {
    // Arrange
    let label = "issue-796-function-style-casts";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT int4('7') AS value", vec![vec![Value::Int64(7)]], 23),
        (
            "SELECT float8('1.5') AS value",
            vec![vec![Value::Float64(1.5)]],
            701,
        ),
        (
            "SELECT text(7) AS value",
            vec![vec![Value::String("7".to_owned())]],
            25,
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(
            |(sql, expected_rows, expected_oid)| match fixture.execute(sql) {
                Ok(result)
                    if result.rows() == expected_rows
                        && result.columns()[0].type_oid == *expected_oid =>
                {
                    None
                }
                Ok(result) => Some(format!(
                    "{sql}: expected {expected_rows:?} with oid {expected_oid}, got {:?} / {}",
                    result.rows(),
                    result.columns()[0].type_oid
                )),
                Err(error) => Some(format!("{sql}: {error}")),
            },
        )
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "function-style casts failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_evaluate_a_function_style_date_cast() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-function-style-date-cast",
        &[
            "CREATE TABLE issue_796_date_cast (value TIMESTAMP)",
            "INSERT INTO issue_796_date_cast VALUES ('2001-02-16 20:38:40')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT DATE(value) AS value FROM issue_796_date_cast");

    // Assert
    let result = result.expect("DATE(timestamp) binds as a type cast");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("2001-02-16".to_owned())]]
    );
    assert_eq!(result.columns()[0].type_oid, 1082);
}

#[test]
fn should_parse_dollar_quoted_constants_in_expression_positions() {
    // Arrange
    let label = "issue-796-dollar-quoted-constant";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        (
            "SELECT $$alpha ' beta$$ AS value",
            vec![vec![Value::String("alpha ' beta".to_owned())]],
        ),
        (
            "SELECT coalesce($tag_2$gamma$tag_2$, 'fallback') AS value",
            vec![vec![Value::String("gamma".to_owned())]],
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_rows)| match fixture.execute(sql) {
            Ok(result) if result.rows() == expected_rows => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected_rows:?}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "dollar-quoted literal cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_parse_postgres_integer_literal_forms() {
    // Arrange
    let label = "issue-796-radix-and-separated-integers";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT 0x2a AS value", 42),
        ("SELECT 0o52 AS value", 42),
        ("SELECT 0b101010 AS value", 42),
        ("SELECT 1_000_000 AS value", 1_000_000),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![Value::Int64(*expected)]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected integer {expected}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "integer literal cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_apply_expression_collation_to_comparisons() {
    // Arrange
    let fixture = sql_fixture("issue-796-expression-collation-comparison", &[]);

    // Act
    let result = fixture.execute("SELECT 'A' COLLATE \"C\" = 'a' COLLATE \"C\"");

    // Assert
    assert_eq!(
        result.expect("collated expression comparison").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_ignore_collation_keywords_inside_expression_comments() {
    // Arrange
    let fixture = sql_fixture("issue-796-collation-keyword-comment", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'A' /* COLLATE \"en-US-x-icu\" */ COLLATE \"C\" = 'A' COLLATE /* comment */ \"C\"",
    );

    // Assert
    assert_eq!(
        result.expect("commented collation keyword is ignored").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_parameter_type_through_expression_collation() {
    // Arrange
    let fixture = sql_fixture("issue-796-expression-collation-parameter", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT $1 COLLATE \"C\"",
        vec![Value::String("hello".into())],
    );

    // Assert
    assert_eq!(
        result.expect("collated parameter expression").rows(),
        &[vec![Value::String("hello".into())]]
    );
}

#[test]
fn should_apply_expression_collation_to_ordering() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-expression-collation-ordering",
        &[
            "CREATE TABLE collation_values (value TEXT)",
            "INSERT INTO collation_values (value) VALUES ('b'), ('A'), ('a')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT value FROM collation_values ORDER BY value COLLATE \"C\"");

    // Assert
    assert_eq!(
        result.expect("collated expression ordering").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("a".to_owned())],
            vec![Value::String("b".to_owned())],
        ]
    );
}

#[test]
fn should_compare_text_under_the_posix_collation() {
    // Arrange
    let fixture = sql_fixture("issue-796-finite-built-in-collations", &[]);

    // Act
    let result = fixture.execute("SELECT 'A' COLLATE POSIX = 'A' COLLATE pg_catalog.\"POSIX\"");

    // Assert
    assert_eq!(
        result.expect("selected POSIX collation form").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_accept_schema_qualified_builtin_collation_form() {
    // Arrange
    let fixture = sql_fixture("issue-796-schema-qualified-builtin-collation", &[]);

    // Act
    let results = [
        fixture.execute("SELECT 'ä' COLLATE POSIX < 'z' COLLATE pg_catalog.\"POSIX\""),
        fixture.execute("SELECT 'ä' COLLATE POSIX < 'z' COLLATE \"pg_catalog\".\"POSIX\""),
    ];

    // Assert
    for result in results {
        assert_eq!(
            result.expect("schema-qualified POSIX collation").rows(),
            &[vec![Value::Bool(false)]]
        );
    }
}

#[test]
fn should_preserve_quoted_schema_case_when_resolving_collations() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-quoted-collation-schema-case",
        &["CREATE TABLE collated_values (value TEXT)"],
    );

    // Act
    let errors = [
        "SELECT 'A' COLLATE \"PG_CATALOG\".\"C\" = 'a'",
        "SELECT value FROM collated_values ORDER BY value COLLATE \"PG_CATALOG\".\"C\"",
    ]
    .map(|sql| fixture.error(sql));

    // Assert
    for error in errors {
        assert!(matches!(
            &error,
            cassie::app::CassieError::Unsupported(message)
                if message.contains("user-defined collations are not selected")
        ));
    }
}

#[test]
fn should_reject_an_unselected_non_c_collation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-non-c-collation-ordering",
        &[
            "CREATE TABLE collation_words (word TEXT)",
            "INSERT INTO collation_words (word) VALUES ('z'), ('ä'), ('a')",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT word FROM collation_words ORDER BY word COLLATE \"en-US-x-icu\"");

    // Assert
    assert!(result
        .expect_err("ICU collation is outside the finite built-in scope")
        .to_string()
        .contains("not selected"));
}

#[test]
fn should_reject_user_defined_nondeterministic_collation_creation() {
    // Arrange
    let fixture = sql_fixture("issue-796-nondeterministic-icu-collation", &[]);

    // Act
    let result = fixture.execute(
        "CREATE COLLATION issue_796_ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
    );

    // Assert
    assert!(result
        .expect_err("user-defined collations are outside scope")
        .to_string()
        .contains("CREATE COLLATION is not supported"));
}

#[test]
fn should_reject_collation_version_refresh() {
    // Arrange
    let fixture = sql_fixture("issue-796-alter-collation-refresh", &[]);

    // Act
    let result = fixture.execute("ALTER COLLATION \"C\" REFRESH VERSION");

    // Assert
    assert!(result
        .expect_err("collation version refresh is outside scope")
        .to_string()
        .contains("ALTER COLLATION is not supported"));
}

#[test]
fn should_reject_column_collation_declarations() {
    // Arrange
    let fixture = sql_fixture("issue-796-column-collation", &[]);

    // Act
    let result = fixture.execute("CREATE TABLE collated_values (value TEXT COLLATE \"C\")");

    // Assert
    assert!(result
        .expect_err("column collation metadata is not selected")
        .to_string()
        .contains("column collation metadata is not supported"));
}

#[test]
fn should_return_boolean_input_validation_results_for_postgres_type_names() {
    // Arrange
    let fixture = sql_fixture("issue-796-input-validity", &[]);

    // Act
    let result = fixture
        .execute("SELECT pg_input_is_valid('42', 'integer'), pg_input_is_valid('bad', 'integer')");

    // Assert
    assert_eq!(
        result.expect("pg_input_is_valid results").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}
