#[test]
fn should_return_every_for_boolean_inputs_with_nulls() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-every-aggregate",
        &[
            "CREATE TABLE issue_806_every_values (flag BOOLEAN)",
            "INSERT INTO issue_806_every_values VALUES (TRUE),(TRUE),(FALSE),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT EVERY(flag) FROM issue_806_every_values");

    // Assert
    assert_eq!(
        result
            .expect("EVERY ignores NULL and returns false for a false input")
            .rows(),
        &[vec![Value::Bool(false)]]
    );
}
