#[test]
fn should_translate_bound_similar_patterns_with_unicode_escape() {
    // Arrange
    let fixture = sql_fixture("issue-799-dynamic-similar-parameters", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT $1 SIMILAR TO $2 ESCAPE $3, $1 NOT SIMILAR TO $2 ESCAPE $3, similar_to_escape($2, $3)",
        vec![Value::String("a_b".into()), Value::String("a💡_b".into()), Value::String("💡".into())],
    );

    // Assert
    assert_eq!(
        result.expect("dynamic SIMILAR parameters").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::String(r"^(?:a\_b)$".into()),
        ]]
    );
}

#[test]
fn should_evaluate_similar_patterns_from_columns() {
    // Arrange
    let fixture = sql_fixture("issue-799-dynamic-similar-columns", &[]);
    fixture.execute("CREATE TABLE pattern_rows (id INT, input_value TEXT, pattern_value TEXT, escape_value TEXT)").expect("pattern fixture");
    fixture.execute("INSERT INTO pattern_rows VALUES (1, 'a_b', 'a!_b', '!'), (2, 'abc', '(a|b)%', ''), (3, 'x', NULL, '!')").expect("three pattern rows");

    // Act
    let result = fixture.execute("SELECT id, input_value SIMILAR TO pattern_value ESCAPE escape_value AS matched FROM pattern_rows ORDER BY id");

    // Assert
    assert_eq!(
        result.expect("column-owned patterns").rows(),
        &[
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(true)],
            vec![Value::Int64(3), Value::Null]
        ]
    );
}

#[test]
fn should_return_oracle_similar_translation_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-helper-text", &[]);

    // Act
    let result = fixture.execute(r#"SELECT similar_to_escape('a%b'), pg_catalog.similar_to_escape('(a|b)+'), similar_to_escape('[a-c_%]'), similar_to_escape('x\'), similar_to_escape('a\"b\"c'), similar_to_escape('[', ''), similar_to_escape('a.b^$')"#);

    // Assert
    assert_eq!(
        result
            .expect("lexical translator does not compile regexes")
            .rows(),
        &[vec![
            Value::String("^(?:a.*b)$".into()),
            Value::String("^(?:(?:a|b)+)$".into()),
            Value::String("^(?:[a-c_%])$".into()),
            Value::String("^(?:x)$".into()),
            Value::String("^(?:a){1,1}?(b){1,1}(?:c)$".into()),
            Value::String("^(?:[)$".into()),
            Value::String(r"^(?:a\.b\^\$)$".into()),
        ]]
    );
}

#[test]
fn should_match_selected_similar_repetition_classes() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-lexical-core", &[]);

    // Act
    let result = fixture.execute(r"SELECT 'a' SIMILAR TO 'a?', '' SIMILAR TO 'a?', 'b' SIMILAR TO '[a-c]', 'aaa' SIMILAR TO 'a{2,3}', 'x' SIMILAR TO 'x\', 'a_b' SIMILAR TO '[a]_[b]', 'a%b' SIMILAR TO 'a\%b'");

    // Assert
    assert_eq!(
        result.expect("selected SIMILAR lexical core").rows(),
        &[vec![Value::Bool(true); 7]]
    );
}

#[test]
fn should_preserve_strict_similar_helper_nulls() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-helper-null", &[]);

    // Act
    let result = fixture.execute("SELECT similar_to_escape(NULL, 'xx'), similar_to_escape('x', NULL), NULL SIMILAR TO 'x', 'x' SIMILAR TO NULL, 'x' SIMILAR TO 'x' ESCAPE NULL");

    // Assert
    assert_eq!(
        result.expect("strict helper NULL behavior").rows(),
        &[vec![Value::Null; 5]]
    );
}

#[test]
fn should_report_similar_escape_errors_before_null_left_results() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-helper-errors", &[]);
    let cases = [
        ("SELECT NULL SIMILAR TO 'x' ESCAPE 'xx'", "22025"),
        (r#"SELECT similar_to_escape('a\"b\"c\"d')"#, "2200C"),
        (r"SELECT 'abab' SIMILAR TO '(ab)\1'", "2201B"),
    ];

    // Act
    let results = cases.map(|(sql, _)| fixture.execute(sql));

    // Assert
    for ((sql, state), result) in cases.into_iter().zip(results) {
        let error = result.expect_err("selected escape error or still-pending ARE");
        let actual = match &error {
            CassieError::Diagnostic(diagnostic) => diagnostic.sql_state,
            CassieError::Execution(message) if message == "invalid escape string" => "22025",
            _ => "unexpected",
        };
        assert_eq!(actual, state, "{sql}: {error:?}");
    }
    assert_eq!(
        fixture
            .execute("SELECT 1::BIGINT")
            .expect("query after errors")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}
