//! PostgreSQL 18 keyword string-function execution contracts tracked by #803.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_nested_keyword_arguments_given_quoted_clause_words_when_text_calls_execute() {
    // Arrange
    let fixture = sql_fixture("text-selection-nested-keywords", &[]);
    let sql = "SELECT SUBSTRING(LOWER('FROM FOR IN') FOR 4 FROM 1), POSITION('for' IN LOWER('FROM FOR IN')), TRIM(BOTH 'é🙂' FROM 'é🙂x🙂é')";

    // Act
    let result = fixture
        .execute(sql)
        .expect("nested expressions stay within their keyword argument");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("from".into()),
            Value::Int64(6),
            Value::String("x".into())
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 25]
    );
}

#[test]
fn should_reject_missing_keyword_operands_given_text_call_syntax_when_statements_are_parsed() {
    // Arrange
    let invalid = [
        "SELECT SUBSTRING('abc' FROM)",
        "SELECT SUBSTRING('abc' FOR)",
        "SELECT POSITION('a' IN)",
        "SELECT TRIM(BOTH FROM)",
    ];

    // Act
    let results = invalid.map(cassie::sql::parse_statement);

    // Assert
    for result in results {
        assert!(
            result.is_err(),
            "a missing keyword operand is never accepted"
        );
    }
}

#[test]
fn should_execute_substring_from_for_syntax() {
    // Arrange
    let fixture = sql_fixture("issue-803-parser-substring-from-for", &[]);

    // Act
    let result = fixture.execute("SELECT SUBSTRING('Thomas' FROM 2 FOR 3)");

    // Assert
    assert_eq!(
        result.expect("SUBSTRING FROM/FOR syntax").rows(),
        &[vec![Value::String("hom".to_owned())]]
    );
}

#[test]
fn should_execute_substring_with_only_a_start_or_count() {
    // Arrange
    let fixture = sql_fixture("issue-803-parser-substring-optional-arguments", &[]);

    // Act
    let result = fixture.execute("SELECT SUBSTRING('Thomas' FROM 3), SUBSTRING('Thomas' FOR 2)");

    // Assert
    assert_eq!(
        result
            .expect("SUBSTRING optional FROM/FOR arguments")
            .rows(),
        &[vec![
            Value::String("omas".to_owned()),
            Value::String("Th".to_owned()),
        ]]
    );
}

#[test]
fn should_execute_position_substring_in_string_syntax() {
    // Arrange
    let fixture = sql_fixture("issue-803-parser-position-in-syntax", &[]);

    // Act
    let result = fixture.execute("SELECT POSITION('om' IN 'Thomas')");

    // Assert
    assert_eq!(
        result.expect("POSITION substring IN string").rows(),
        &[vec![Value::Int64(3)]]
    );
}

#[test]
fn should_execute_trim_directions_with_explicit_character_sets() {
    // Arrange
    let fixture = sql_fixture("issue-803-parser-trim-directions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT TRIM(LEADING 'xy' FROM 'xyTitle'), TRIM(TRAILING 'xy' FROM 'Titlexy'), TRIM(BOTH 'xy' FROM 'xyTitlexy')",
    );

    // Assert
    assert_eq!(
        result
            .expect("TRIM direction and character set syntax")
            .rows(),
        &[vec![
            Value::String("Title".to_owned()),
            Value::String("Title".to_owned()),
            Value::String("Title".to_owned()),
        ]]
    );
}

#[test]
fn should_execute_trim_directions_with_default_space_sets() {
    // Arrange
    let fixture = sql_fixture("issue-803-parser-trim-default-spaces", &[]);

    // Act
    let result =
        fixture.execute("SELECT TRIM(LEADING FROM '  title  '), TRIM(TRAILING FROM '  title  ')");

    // Assert
    assert_eq!(
        result
            .expect("TRIM directions with default space set")
            .rows(),
        &[vec![
            Value::String("title  ".to_owned()),
            Value::String("  title".to_owned()),
        ]]
    );
}
