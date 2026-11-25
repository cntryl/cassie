#[test]
fn should_project_primary_key_functional_dependencies_with_grouped_aggregates() {
    // Arrange
    let fixture = grouping_fixture("issue-804-grouping-functional-dependency");

    // Act
    let result = fixture.execute(
        "SELECT id, label, SUM(amount) FROM issue_804_group_values GROUP BY id ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("primary-key dependent projection").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("e".to_owned()),
                Value::Int64(2)
            ],
            vec![
                Value::Int64(2),
                Value::String("e".to_owned()),
                Value::Int64(3)
            ],
            vec![
                Value::Int64(3),
                Value::String("w".to_owned()),
                Value::Int64(5)
            ],
        ]
    );
}
