use super::{rename_field_selections, FieldRename};

#[test]
fn should_rename_a_field_selection_and_keep_the_output_column_name() {
    // Arrange
    let sql = "SELECT (v).b FROM t";

    // Act
    let renamed = rename_field_selections(sql, "b", "bb");

    // Assert
    assert_eq!(
        renamed,
        FieldRename {
            query: Some("SELECT (v).bb AS b FROM t".to_owned()),
            ambiguous: false,
        }
    );
}

#[test]
fn should_rename_a_field_selection_outside_the_select_list_without_an_alias() {
    // Arrange
    let sql = "SELECT id FROM t WHERE (v).b > 0 ORDER BY (v).b";

    // Act
    let renamed = rename_field_selections(sql, "b", "bb");

    // Assert
    assert_eq!(
        renamed.query.as_deref(),
        Some("SELECT id FROM t WHERE (v).bb > 0 ORDER BY (v).bb")
    );
}

#[test]
fn should_keep_an_explicit_alias_of_a_renamed_field_selection() {
    // Arrange
    let sql = "SELECT (t.v).b AS label, length((v).b) FROM t";

    // Act
    let renamed = rename_field_selections(sql, "b", "bb");

    // Assert
    assert_eq!(
        renamed.query.as_deref(),
        Some("SELECT (t.v).bb AS label, length((v).bb) FROM t")
    );
}

#[test]
fn should_leave_string_literals_and_other_fields_untouched() {
    // Arrange
    let sql = "SELECT '(v).b' AS s, (v).a FROM t";

    // Act
    let renamed = rename_field_selections(sql, "b", "bb");

    // Assert
    assert_eq!(
        renamed,
        FieldRename {
            query: None,
            ambiguous: false,
        }
    );
}

#[test]
fn should_report_a_dotted_reference_that_is_not_a_field_selection() {
    // Arrange
    let sql = "SELECT r.b FROM (SELECT (v).* FROM t) r";

    // Act
    let renamed = rename_field_selections(sql, "b", "bb");

    // Assert
    assert!(renamed.ambiguous);
}

#[test]
fn should_match_a_quoted_attribute_name_exactly() {
    // Arrange
    let sql = "SELECT (v).\"B\", (v).b FROM t";

    // Act
    let renamed = rename_field_selections(sql, "B", "C");

    // Assert
    assert_eq!(
        renamed.query.as_deref(),
        Some("SELECT (v).\"C\" AS \"B\", (v).b FROM t")
    );
}
