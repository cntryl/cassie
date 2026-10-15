#[test]
fn should_report_owner_privileges_in_information_schema_role_table_grants() {
    // Arrange
    // The created relation is owned by the current test session; no extra role or grant is needed.

    // Act
    assert_owner_privileges_in_view(
        "role_table_grants",
        "issue-817-information-schema-role-table-grants",
    );

    // Assert
    // The helper asserts the relation owner, seven privileges, and their descriptors.
}

#[test]
fn should_report_owner_privileges_in_information_schema_table_privileges() {
    // Arrange
    // The created relation is owned by the current test session; no extra role or grant is needed.

    // Act
    assert_owner_privileges_in_view(
        "table_privileges",
        "issue-817-information-schema-table-privileges",
    );

    // Assert
    // The helper asserts the relation owner, seven privileges, and their descriptors.
}
