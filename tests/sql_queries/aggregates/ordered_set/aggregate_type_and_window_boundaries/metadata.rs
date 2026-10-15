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
