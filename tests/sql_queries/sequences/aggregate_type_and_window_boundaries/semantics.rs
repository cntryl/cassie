#[test]
fn should_preserve_mode_result_identity_for_non_int4_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-mode-result-type-identity",
        &[
            "CREATE TABLE issue_806_mode_type_values (small_value SMALLINT, big_value BIGINT, text_value TEXT)",
            "INSERT INTO issue_806_mode_type_values VALUES (-1,10000000000,'z'),(-1,10000000000,'z'),(2,20000000000,'a')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT mode() WITHIN GROUP (ORDER BY small_value), mode() WITHIN GROUP (ORDER BY big_value), mode() WITHIN GROUP (ORDER BY text_value) FROM issue_806_mode_type_values",
    );

    // Assert
    let result = result.expect("mode returns the ordering expression's result type");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(-1),
            Value::Int64(10_000_000_000),
            Value::String("z".to_owned()),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![21, 20, 25]
    );
}
