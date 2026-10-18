#[test]
fn should_return_statistical_regression_aggregate_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-regression-aggregates",
        &[
            "CREATE TABLE issue_806_regression_values (x INT, y INT)",
            "INSERT INTO issue_806_regression_values VALUES (1,2),(2,4),(3,6)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT corr(CAST(x AS FLOAT8), CAST(y AS FLOAT8)), covar_pop(CAST(x AS FLOAT8), CAST(y AS FLOAT8)), covar_samp(CAST(x AS FLOAT8), CAST(y AS FLOAT8)), regr_count(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_slope(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_intercept(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_r2(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_sxx(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_sxy(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_syy(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_avgx(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_avgy(CAST(y AS FLOAT8), CAST(x AS FLOAT8)) FROM issue_806_regression_values",
    );

    // Assert
    assert_eq!(
        result
            .expect("statistical and regression aggregates")
            .rows(),
        &[vec![
            Value::Float64(1.0),
            Value::Float64(4.0 / 3.0),
            Value::Float64(2.0),
            Value::Int64(3),
            Value::Float64(2.0),
            Value::Float64(0.0),
            Value::Float64(1.0),
            Value::Float64(2.0),
            Value::Float64(4.0),
            Value::Float64(8.0),
            Value::Float64(2.0),
            Value::Float64(4.0),
        ]]
    );
}
