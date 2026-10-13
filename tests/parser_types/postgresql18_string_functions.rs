//! PostgreSQL 18 keyword string-function execution contracts tracked by #803.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

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
