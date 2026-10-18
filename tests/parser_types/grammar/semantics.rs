#[test]
fn should_accept_postgres_float_type_aliases() {
    // Arrange
    let label = "issue-796-float-type-aliases";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('1.25' AS float4) AS value", 700),
        ("SELECT CAST('1.25' AS float8) AS value", 701),
        ("SELECT CAST('1.25' AS real) AS value", 700),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::Float64(1.25)]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value 1.25, got {:?} / {}",
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
fn should_order_text_with_a_non_c_postgres_collation() {
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
    assert_eq!(
        result.expect("ICU English collation ordering").rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String("ä".to_owned())],
            vec![Value::String("z".to_owned())],
        ]
    );
}

#[test]
fn should_create_a_nondeterministic_postgres_icu_collation() {
    // Arrange
    let fixture = sql_fixture("issue-796-nondeterministic-icu-collation", &[]);

    // Act
    fixture
        .execute(
            "CREATE COLLATION issue_796_ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
        )
        .expect("create a nondeterministic ICU collation");
    let result = fixture.execute("SELECT 'A' = 'a' COLLATE issue_796_ci");

    // Assert
    assert_eq!(
        result
            .expect("case-insensitive collation comparison")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_refresh_a_postgres_collation_version() {
    // Arrange
    let fixture = sql_fixture("issue-796-alter-collation-refresh", &[]);

    // Act
    let result = fixture.execute("ALTER COLLATION \"C\" REFRESH VERSION");

    // Assert
    result.expect("refresh the recorded version of a PostgreSQL collation");
}

#[test]
fn should_apply_column_collation_to_distinct_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-column-collation-distinct",
        &[
            "CREATE TABLE collated_values (value TEXT COLLATE \"C\")",
            "INSERT INTO collated_values (value) VALUES ('A'), ('a'), ('A')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT DISTINCT value FROM collated_values ORDER BY value");

    // Assert
    assert_eq!(
        result.expect("collation-aware DISTINCT").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("a".to_owned())],
        ]
    );
}

#[test]
fn should_apply_column_collation_to_group_keys() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-column-collation-grouping",
        &[
            "CREATE TABLE collated_values (value TEXT COLLATE \"C\")",
            "INSERT INTO collated_values (value) VALUES ('A'), ('a'), ('A')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT value, COUNT(*) FROM collated_values GROUP BY value");

    // Assert
    assert_eq!(result.expect("collation-aware GROUP BY").rows().len(), 2);
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
