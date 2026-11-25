#[test]
fn should_bind_text_parameters_in_ilike_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-parameter", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT 'Alice' ILIKE $1 AS matched",
        vec![Value::String("a%".to_owned())],
    );

    // Assert
    assert_eq!(
        result.expect("bound ILIKE pattern").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_bind_text_parameters_in_regex_operator_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-parameter", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT 'Alice' ~ $1 AS matched",
        vec![Value::String("lic".to_owned())],
    );

    // Assert
    assert_eq!(
        result.expect("bound regex operator pattern").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_bind_regexp_replace_overloaded_parameters() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-parameters", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT regexp_replace($1, $2, $3, $4), regexp_replace($1, $2, $3, $5::INT)",
        vec![
            Value::String("abcabc".to_owned()),
            Value::String("a".to_owned()),
            Value::String("X".to_owned()),
            Value::String("g".to_owned()),
            Value::Int64(2),
        ],
    );

    // Assert
    assert_eq!(
        result.expect("bound regexp_replace overload parameters").rows(),
        &[vec![
            Value::String("XbcXbc".to_owned()),
            Value::String("abcXbc".to_owned()),
        ]]
    );
}
