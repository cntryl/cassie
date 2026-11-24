#[test]
fn should_skip_sql_nulls_but_keep_json_nulls_in_strict_jsonb_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-jsonb-agg-strict-null-policy",
        &[
            "CREATE TABLE issue_811_strict_json_values (payload JSONB)",
            "INSERT INTO issue_811_strict_json_values VALUES (NULL), ('null')",
        ],
    );
    let sql = "SELECT jsonb_agg_strict(payload)::TEXT FROM issue_811_strict_json_values";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("strict JSONB aggregate null policy").rows(),
        &[vec![Value::String("[null]".to_owned())]]
    );
}

#[test]
fn should_include_sql_null_inputs_in_non_strict_jsonb_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-jsonb-aggregate-sql-null-inputs",
        &[
            "CREATE TABLE issue_811_jsonb_aggregate_values (payload JSONB)",
            "INSERT INTO issue_811_jsonb_aggregate_values VALUES ('1'), (NULL), ('2')",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT jsonb_agg(payload)::TEXT FROM issue_811_jsonb_aggregate_values");

    // Assert
    assert_eq!(
        result
            .expect("non-strict JSONB aggregate keeps SQL NULL inputs")
            .rows(),
        &[vec![Value::String("[1, null, 2]".to_owned())]]
    );
}
