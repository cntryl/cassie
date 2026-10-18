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

#[test]
fn should_apply_column_collation_to_like_matching() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-column-collation-like",
        &[
            "CREATE TABLE collated_values (value TEXT COLLATE \"C\")",
            "INSERT INTO collated_values (value) VALUES ('A'), ('a'), ('A')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT value FROM collated_values WHERE value LIKE 'A'");

    // Assert
    assert_eq!(
        result.expect("collation-aware LIKE").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("A".to_owned())]
        ]
    );
}
