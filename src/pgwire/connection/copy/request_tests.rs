use super::{parse_copy_request, split_copy_where};

#[test]
fn should_parse_copy_where_at_the_start_of_the_option_tail() {
    // Arrange
    let sql = "COPY copy_unknown_where FROM STDIN WHERE (\"missing_é\" = 1)";

    // Act
    let request = parse_copy_request(sql).expect("valid COPY").expect("COPY request");

    // Assert
    assert_eq!(request.where_clause.as_deref(), Some("\"missing_é\" = 1"));
}

#[test]
fn should_ignore_quoted_nested_and_non_boundary_where_text() {
    // Arrange
    let options = "WITH (FORMAT CSV, NULL ' WHERE ', FORCE_NULL (\"where\")) WHERE (id = 1)";
    let no_boundary = "WHEREVER id = 1";
    let no_space = "WHERE(id = 1)";

    // Act
    let (prefix, predicate) = split_copy_where(options);

    // Assert
    assert_eq!(prefix, "WITH (FORMAT CSV, NULL ' WHERE ', FORCE_NULL (\"where\"))");
    assert_eq!(predicate.as_deref(), Some("id = 1"));
    assert_eq!(split_copy_where(no_boundary), (no_boundary, None));
    assert_eq!(split_copy_where(no_space), (no_space, None));
}

#[test]
fn should_keep_a_missing_copy_target_on_the_existing_error_path() {
    // Arrange
    let sql = "COPY FROM STDIN";

    // Act
    let error = parse_copy_request(sql).expect_err("missing target");

    // Assert
    assert!(matches!(
        error,
        crate::app::CassieError::Parse(message)
            if message == "COPY requires FROM STDIN or TO STDOUT"
    ));
}
