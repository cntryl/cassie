#[test]
fn should_preserve_split_part_empty_field_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-803-split-part-empty-null-delimiters", &[]);

    // Act
    let result = fixture.execute(
        "SELECT split_part('abc', '', 1), split_part('abc', '', 2), split_part('abc', NULL, 1), split_part('a,,b,', ',', 2), split_part('a,,b,', ',', 4)",
    );

    // Assert
    assert_eq!(
        result.expect("empty and NULL split_part delimiters").rows(),
        &[vec![
            Value::String("abc".to_owned()),
            Value::String(String::new()),
            Value::Null,
            Value::String(String::new()),
            Value::String(String::new()),
        ]]
    );
}

#[test]
fn should_preserve_null_quote_literal_separately_from_quote_nullable() {
    // Arrange
    let fixture = sql_fixture("issue-803-quote-literal-null", &[]);

    // Act
    let result = fixture
        .execute("SELECT quote_literal(CAST(NULL AS TEXT)), quote_nullable(CAST(NULL AS TEXT))");

    // Assert
    assert_eq!(
        result.expect("NULL literal quoting forms").rows(),
        &[vec![Value::Null, Value::String("NULL".to_owned())]]
    );
}

#[test]
fn should_return_zero_for_empty_ascii_input() {
    // Arrange
    let fixture = sql_fixture("issue-803-ascii-empty-string", &[]);

    // Act
    let result = fixture.execute("SELECT ascii('')");

    // Assert
    assert_eq!(
        result.expect("empty ascii input").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_propagate_null_through_selected_text_transform_signatures() {
    // Arrange
    let fixture = sql_fixture("issue-1034-text-transform-nulls", &[]);

    // Act
    let result = fixture.execute(
        "SELECT overlay(CAST(NULL AS TEXT) PLACING 'x' FROM 1), lpad(CAST(NULL AS TEXT), 2), rpad('x', CAST(NULL AS INT)), repeat(CAST(NULL AS TEXT), 2), reverse(CAST(NULL AS TEXT)), initcap(CAST(NULL AS TEXT)), translate(CAST(NULL AS TEXT), 'a', 'b'), concat_ws(CAST(NULL AS TEXT), 'x')",
    );

    // Assert
    assert_eq!(
        result
            .expect("strict transform functions and NULL separator")
            .rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]]
    );
}
