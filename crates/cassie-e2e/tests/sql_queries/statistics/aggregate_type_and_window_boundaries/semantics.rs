#[test]
fn should_define_regression_statistics_for_zero_or_one_pairs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-regression-zero-one-pair-results",
        &[
            "CREATE TABLE issue_806_regression_edge_values (x FLOAT8, y FLOAT8)",
            "INSERT INTO issue_806_regression_edge_values VALUES (2,4),(NULL,3),(2,NULL)",
        ],
    );
    let query = "SELECT corr(y,x), covar_pop(y,x), covar_samp(y,x), regr_count(y,x), regr_slope(y,x), regr_intercept(y,x), regr_r2(y,x), regr_sxx(y,x), regr_sxy(y,x), regr_syy(y,x), regr_avgx(y,x), regr_avgy(y,x) FROM issue_806_regression_edge_values";

    // Act
    let empty = fixture.execute(&format!("{query} WHERE FALSE"));
    let one_pair = fixture.execute(&format!("{query} WHERE x = 2 AND y = 4"));

    // Assert
    let expected_oids = vec![701, 701, 701, 20, 701, 701, 701, 701, 701, 701, 701, 701];
    let empty = empty.expect("regression aggregates over an empty input");
    assert_eq!(
        empty.rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int64(0),
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
    assert_eq!(
        empty
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );

    let one_pair = one_pair.expect("regression aggregates over one non-NULL pair");
    assert_eq!(
        one_pair.rows(),
        &[vec![
            Value::Null,
            Value::Float64(0.0),
            Value::Null,
            Value::Int64(1),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Float64(0.0),
            Value::Float64(0.0),
            Value::Float64(0.0),
            Value::Float64(2.0),
            Value::Float64(4.0),
        ]]
    );
    assert_eq!(
        one_pair
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );
}
