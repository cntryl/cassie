#[test]
fn should_filter_rows_with_a_tagged_dollar_quoted_constant() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-dollar-quoted-predicate",
        &[
            "CREATE TABLE dollar_quoted_rows (id BIGINT, value TEXT)",
            "INSERT INTO dollar_quoted_rows VALUES (1, 'alpha '' beta'), (2, 'gamma')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id FROM dollar_quoted_rows WHERE value = $tag_2$alpha ' beta$tag_2$ ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("tagged dollar-quoted predicate literal")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_evaluate_symmetric_between_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-796-between-symmetric", &[]);

    // Act
    let result =
        fixture.execute("SELECT 2 BETWEEN SYMMETRIC 3 AND 1, 4 NOT BETWEEN SYMMETRIC 3 AND 1");

    // Assert
    assert_eq!(
        result.expect("symmetric BETWEEN predicates").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_is_truth_value_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-796-truth-predicates", &[]);

    // Act
    let result = fixture
        .execute("SELECT TRUE IS TRUE, FALSE IS NOT TRUE, NULL IS UNKNOWN, TRUE IS NOT UNKNOWN");

    // Assert
    assert_eq!(
        result.expect("truth-value predicates").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
