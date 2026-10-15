#[test]
fn should_group_by_a_select_output_alias() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-alias");

    // Act
    let by_alias = fixture.execute(
        "SELECT region AS area, SUM(amount) FROM issue_804_group_values GROUP BY area ORDER BY area",
    );

    // Assert
    assert_eq!(
        by_alias.expect("GROUP BY output alias").rows(),
        &[
            vec![Value::String("east".to_owned()), Value::Int64(5)],
            vec![Value::String("west".to_owned()), Value::Int64(5)],
        ]
    );
}

#[test]
fn should_group_by_a_select_list_ordinal() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-ordinal");

    // Act
    let by_ordinal = fixture
        .execute("SELECT region, SUM(amount) FROM issue_804_group_values GROUP BY 1 ORDER BY 1");

    // Assert
    assert_eq!(
        by_ordinal.expect("GROUP BY output ordinal").rows(),
        &[
            vec![Value::String("east".to_owned()), Value::Int64(5)],
            vec![Value::String("west".to_owned()), Value::Int64(5)],
        ]
    );
}
