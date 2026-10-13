//! PostgreSQL 18 keyword string-function grammar tracked by #803.

use cassie::sql::ast::QueryStatement;

fn parsed_projection_count(sql: &str) -> usize {
    let parsed = cassie::sql::parse_statement(sql).expect("string expression parses");
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT statement");
    };
    statement.projection.len()
}

#[test]
fn should_parse_substring_from_for_syntax() {
    // Arrange
    let sql = "SELECT SUBSTRING('Thomas' FROM 2 FOR 3)";

    // Act
    let columns = parsed_projection_count(sql);

    // Assert
    assert_eq!(columns, 1);
}

#[test]
fn should_parse_substring_with_only_a_start_or_only_a_count() {
    // Arrange
    let sql = "SELECT SUBSTRING('Thomas' FROM 3), SUBSTRING('Thomas' FOR 2)";

    // Act
    let columns = parsed_projection_count(sql);

    // Assert
    assert_eq!(columns, 2);
}

#[test]
fn should_parse_position_substring_in_string_syntax() {
    // Arrange
    let sql = "SELECT POSITION('om' IN 'Thomas')";

    // Act
    let columns = parsed_projection_count(sql);

    // Assert
    assert_eq!(columns, 1);
}

#[test]
fn should_parse_trim_directions() {
    // Arrange
    let sql = "SELECT TRIM(LEADING 'xy' FROM 'xyTitle'), TRIM(TRAILING 'xy' FROM 'Titlexy'), TRIM(BOTH 'xy' FROM 'xyTitlexy')";

    // Act
    let columns = parsed_projection_count(sql);

    // Assert
    assert_eq!(columns, 3);
}

#[test]
fn should_parse_trim_direction_with_default_space_set() {
    // Arrange
    let sql = "SELECT TRIM(LEADING FROM '  title  '), TRIM(TRAILING FROM '  title  ')";

    // Act
    let columns = parsed_projection_count(sql);

    // Assert
    assert_eq!(columns, 2);
}
