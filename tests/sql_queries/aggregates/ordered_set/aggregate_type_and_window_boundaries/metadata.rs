#[test]
fn should_preserve_ordered_set_percentile_array_results_with_exact_oids() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-ordered-set-array-result-oids",
        &[
            "CREATE TABLE issue_806_percentile_array_types (value SMALLINT)",
            "INSERT INTO issue_806_percentile_array_types VALUES (1),(2),(3),(4)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT percentile_cont(ARRAY[0.25,0.5,1.0]) WITHIN GROUP (ORDER BY value::FLOAT8), percentile_disc(ARRAY[0.25,0.5,1.0]) WITHIN GROUP (ORDER BY value) FROM issue_806_percentile_array_types",
    );

    // Assert
    let result = result.expect("ordered-set percentile arrays preserve their output identities");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Json(serde_json::json!([1.75, 2.5, 4.0])),
            Value::Json(serde_json::json!([1, 2, 4])),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1022, 1005]
    );
}

#[test]
fn should_preserve_string_carried_percentile_array_metadata() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-ordered-set-string-array-result-oids",
        &[
            "CREATE TABLE issue_806_percentile_string_types (t TEXT, v VARCHAR(20))",
            "INSERT INTO issue_806_percentile_string_types VALUES ('alpha','first'),('zeta','last')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT percentile_disc(ARRAY[0.0,NULL,1.0]) WITHIN GROUP (ORDER BY t), percentile_disc(ARRAY[0.0,NULL,1.0]) WITHIN GROUP (ORDER BY v) FROM issue_806_percentile_string_types",
    );

    // Assert
    let result = result.expect("string-carried percentile arrays");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Json(serde_json::json!(["alpha", null, "zeta"])),
            Value::Json(serde_json::json!(["first", null, "last"])),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1009, 1015]
    );
}

#[test]
fn should_preserve_scalar_discrete_array_metadata() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-discrete-native-array-result-oids",
        &[
            "CREATE TABLE issue_806_discrete_array_types (numbers INTEGER[], flags BOOLEAN[])",
            "INSERT INTO issue_806_discrete_array_types VALUES ('[0:2]={1,NULL,3}'::INTEGER[], ARRAY[true,NULL,false])",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY numbers), percentile_disc(0.5) WITHIN GROUP (ORDER BY flags) FROM issue_806_discrete_array_types",
    );

    // Assert
    let result = result.expect("scalar SQL Array percentile results");
    let numbers = cassie::types::SqlArray::new(
        vec![cassie::types::ArrayDimension {
            length: 3,
            lower_bound: 0,
        }],
        vec![Value::Int64(1), Value::Null, Value::Int64(3)],
    )
    .expect("numbers");
    let flags = cassie::types::SqlArray::from_elements(vec![
        Value::Bool(true),
        Value::Null,
        Value::Bool(false),
    ])
    .expect("flags");
    assert_eq!(
        result.rows(),
        &[vec![Value::Array(numbers), Value::Array(flags)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1007, 1000]
    );
}
