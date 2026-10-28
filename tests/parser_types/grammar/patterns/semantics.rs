#[test]
fn should_resolve_unicode_escaped_identifiers() {
    // Arrange
    let fixture = sql_fixture("issue-796-unicode-escaped-identifier", &[]);

    // Act
    let created = fixture.execute("CREATE TABLE unicode_names (U&\"na!006de\" UESCAPE '!' TEXT)");
    let inserted = fixture.execute("INSERT INTO unicode_names (name) VALUES ('resolved')");
    let selected = fixture.execute("SELECT name FROM unicode_names");

    // Assert
    created.expect("Unicode escaped identifier DDL");
    inserted.expect("Unicode escaped identifier binding");
    assert_eq!(
        selected.expect("Unicode escaped identifier lookup").rows(),
        &[vec![Value::String("resolved".to_owned())]]
    );
}

#[test]
fn should_apply_custom_unicode_escape_to_quoted_identifiers() {
    // Arrange
    let fixture = sql_fixture("issue-796-custom-unicode-identifier-escape", &[]);

    // Act
    let created = fixture.execute("CREATE TABLE U&\"issue_796_!0065scape\" UESCAPE '!' (id INT)");
    let inserted = fixture.execute("INSERT INTO issue_796_escape VALUES (42)");
    let selected =
        fixture.execute("SELECT id FROM U&\"issue_796_!0065scape\" UESCAPE '!' ORDER BY id");

    // Assert
    created.expect("create a table using a custom Unicode identifier escape");
    inserted.expect("insert through the decoded identifier spelling");
    assert_eq!(
        selected
            .expect("resolve the custom-escaped identifier on a later query")
            .rows(),
        &[vec![Value::Int64(42)]]
    );
}
