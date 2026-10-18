#[test]
fn should_filter_regression_rows_pairwise_when_either_input_is_null() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-regression-pair-null-filter",
        &[
            "CREATE TABLE issue_806_regression_pair_values (x INT, y INT)",
            "INSERT INTO issue_806_regression_pair_values VALUES (1,2),(2,6),(NULL,10),(4,NULL),(5,10)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT corr(CAST(x AS FLOAT), CAST(y AS FLOAT)), covar_pop(CAST(x AS FLOAT), CAST(y AS FLOAT)), covar_samp(CAST(x AS FLOAT), CAST(y AS FLOAT)), regr_count(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_slope(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_intercept(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_r2(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_sxx(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_sxy(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_syy(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_avgx(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_avgy(CAST(y AS FLOAT), CAST(x AS FLOAT)) FROM issue_806_regression_pair_values",
    );

    // Assert
    let result = result.expect("statistical aggregates must share pairwise NULL filtering");
    let row = &result.rows()[0];
    assert_float_near(&row[0], 0.960_768_922_830_522_8);
    assert_float_near(&row[1], 16.0 / 3.0);
    assert_float_near(&row[2], 8.0);
    assert_eq!(row[3], Value::Int64(3));
    assert_float_near(&row[4], 24.0 / 13.0);
    assert_float_near(&row[5], 14.0 / 13.0);
    assert_float_near(&row[6], 12.0 / 13.0);
    assert_float_near(&row[7], 26.0 / 3.0);
    assert_float_near(&row[8], 16.0);
    assert_float_near(&row[9], 32.0);
    assert_float_near(&row[10], 8.0 / 3.0);
    assert_float_near(&row[11], 6.0);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701, 701, 20, 701, 701, 701, 701, 701, 701, 701, 701]
    );
}

#[test]
fn should_return_population_zero_for_singleton_statistics() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-singleton",
        &[
            "CREATE TABLE issue_806_statistical_singleton_values (n FLOAT)",
            "INSERT INTO issue_806_statistical_singleton_values VALUES (7),(NULL)",
        ],
    );

    // Act
    let singleton = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_pop(n), stddev_samp(n) FROM issue_806_statistical_singleton_values WHERE n = 7",
    );
    let empty = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_pop(n), stddev_samp(n) FROM issue_806_statistical_singleton_values WHERE FALSE",
    );

    // Assert
    let singleton = singleton.expect("sample and population statistics over one value");
    assert_eq!(
        singleton.rows(),
        &[vec![
            Value::Float64(0.0),
            Value::Null,
            Value::Float64(0.0),
            Value::Null,
        ]]
    );
    let empty = empty.expect("sample and population statistics over no values");
    assert_eq!(
        empty.rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
    assert_eq!(
        singleton
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701, 701, 701]
    );
}

#[test]
fn should_distinguish_sample_from_population_statistics_for_multiple_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-sample-population-denominators",
        &[
            "CREATE TABLE issue_806_statistical_denominators (n FLOAT8)",
            "INSERT INTO issue_806_statistical_denominators VALUES (1), (3), (5), (NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_pop(n), stddev_samp(n) \
         FROM issue_806_statistical_denominators",
    );

    // Assert
    let result = result.expect("sample and population statistics over multiple values");
    assert_eq!(result.rows().len(), 1);
    let row = &result.rows()[0];
    assert_float_near(&row[0], 8.0 / 3.0);
    assert_float_near(&row[1], 4.0);
    assert_float_near(&row[2], (8.0_f64 / 3.0).sqrt());
    assert_float_near(&row[3], 2.0);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701; 4]
    );
}
