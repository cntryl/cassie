#[test]
fn should_define_statistical_aggregate_results_for_nonfinite_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-806-nonfinite-statistical-inputs", &[]);
    let query = "SELECT corr(x,y), covar_pop(x,y), covar_samp(x,y), regr_count(y,x), regr_slope(y,x), regr_intercept(y,x), regr_r2(y,x), regr_sxx(y,x), regr_sxy(y,x), regr_syy(y,x), regr_avgx(y,x), regr_avgy(y,x) FROM (VALUES ({x}::FLOAT8,1::FLOAT8),(1,2),(2,3)) AS pairs(x,y)";
    let nan = fixture.execute(&query.replace("{x}", "'NaN'"));
    let infinity = fixture.execute(&query.replace("{x}", "'Infinity'"));

    // Act
    let nan = nan.expect("statistical aggregates over NaN input");
    let infinity = infinity.expect("statistical aggregates over positive infinity input");

    // Assert
    let expected_oids = vec![701, 701, 701, 20, 701, 701, 701, 701, 701, 701, 701, 701];
    for result in [&nan, &infinity] {
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            expected_oids
        );
        assert_eq!(result.rows()[0][3], Value::Int64(3));
        assert_eq!(result.rows()[0][9], Value::Float64(2.0));
        assert_eq!(result.rows()[0][11], Value::Float64(2.0));
        for index in [0, 1, 2, 4, 5, 6, 7, 8] {
            assert!(
                matches!(&result.rows()[0][index], Value::Float64(value) if value.is_nan()),
                "statistical output at index {index} should be NaN: {:?}",
                result.rows()[0][index]
            );
        }
    }
    assert!(matches!(
        &nan.rows()[0][10],
        Value::Float64(value) if value.is_nan()
    ));
    assert!(matches!(
        &infinity.rows()[0][10],
        Value::Float64(value) if value.is_infinite() && value.is_sign_positive()
    ));
}
