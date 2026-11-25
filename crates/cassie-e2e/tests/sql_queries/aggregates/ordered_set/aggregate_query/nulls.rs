#[test]
fn should_interpolate_continuous_percentiles_after_null_filtering() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-percentile-interpolation",
        &[
            "CREATE TABLE issue_806_percentile_values (n INT)",
            "INSERT INTO issue_806_percentile_values VALUES (1),(3),(NULL),(5),(7)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT PERCENTILE_CONT(0.25) WITHIN GROUP (ORDER BY n::FLOAT8), PERCENTILE_DISC(0.25) WITHIN GROUP (ORDER BY n), COUNT(n) FROM issue_806_percentile_values",
    );

    // Assert
    let result = result.expect("continuous and discrete percentiles with a NULL input");
    assert_eq!(
        result.rows(),
        &[vec![Value::Float64(2.5), Value::Int64(1), Value::Int64(4)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 23, 20]
    );
}
