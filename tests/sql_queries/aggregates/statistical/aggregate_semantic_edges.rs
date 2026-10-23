use cassie::types::extended::numeric::{math as numeric_math, parse_text, Numeric};
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
fn should_apply_regression_filter_before_evaluating_both_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-regression-filter-order",
        &[
            "CREATE TABLE issue_806_regression_filter_values (x INT, y INT)",
            "INSERT INTO issue_806_regression_filter_values VALUES (1,2),(2,4)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT regr_count(CAST(y AS FLOAT8), CAST(1 / (x - x) AS FLOAT8)) \
         FILTER (WHERE y < 0) FROM issue_806_regression_filter_values",
    );

    // Assert
    assert_eq!(
        result.expect("FILTER must short-circuit both regression inputs").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_coerce_numeric_regression_inputs_to_float8() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-numeric-inputs", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regr_avgx(y, x), regr_avgy(y, x) \
         FROM (VALUES (2::NUMERIC, 1::NUMERIC), (4::NUMERIC, 2::NUMERIC)) AS pairs(y, x)",
    );

    // Assert
    let result = result.expect("numeric regression inputs cast to FLOAT8");
    assert_eq!(result.rows(), &[vec![Value::Float64(1.5), Value::Float64(3.0)]]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701]
    );
}

#[test]
fn should_return_horizontal_line_regression_results() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-horizontal-regression-line",
        &[
            "CREATE TABLE issue_806_horizontal_regression_values (x FLOAT8, y FLOAT8)",
            "INSERT INTO issue_806_horizontal_regression_values VALUES (1,5),(2,5),(3,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT corr(y, x), regr_slope(y, x), regr_intercept(y, x), regr_r2(y, x) \
         FROM issue_806_horizontal_regression_values",
    );

    // Assert
    assert_eq!(
        result.expect("horizontal regression line").rows(),
        &[vec![
            Value::Null,
            Value::Float64(0.0),
            Value::Float64(5.0),
            Value::Float64(1.0),
        ]]
    );
}

#[test]
fn should_return_null_results_for_vertical_line_regression() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-vertical-regression-line",
        &[
            "CREATE TABLE issue_806_vertical_regression_values (x FLOAT8, y FLOAT8)",
            "INSERT INTO issue_806_vertical_regression_values VALUES (7,1),(7,2),(7,3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT corr(y, x), regr_slope(y, x), regr_intercept(y, x), regr_r2(y, x) \
         FROM issue_806_vertical_regression_values",
    );

    // Assert
    assert_eq!(
        result.expect("vertical regression line").rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
}

#[test]
fn should_reject_numeric_regression_input_outside_float8_range() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-numeric-range", &[]);

    // Act
    let results = [
        fixture.execute("SELECT regr_count(1::NUMERIC, 1e10000::NUMERIC)"),
        fixture.execute("SELECT regr_count(1::NUMERIC, 1e-10000::NUMERIC)"),
    ];

    // Assert
    for result in &results {
        assert!(matches!(
            result,
            Err(cassie::app::CassieError::FloatInput(
                cassie::types::FloatInputError::Range
            ))
        ), "unexpected regression result: {result:?}");
    }
}

#[test]
fn should_propagate_first_row_nonfinite_regression_moments() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-nonfinite-first-row", &[]);

    // Act
    let x_infinity = fixture.execute(
        "SELECT regr_sxx(1::FLOAT8, 'Infinity'::FLOAT8), \
         regr_sxy(1::FLOAT8, 'Infinity'::FLOAT8)",
    );
    let y_nan = fixture.execute(
        "SELECT regr_syy('NaN'::FLOAT8, 1::FLOAT8), \
         regr_sxy('NaN'::FLOAT8, 1::FLOAT8)",
    );

    // Assert
    for result in [x_infinity, y_nan] {
        let result = result.expect("non-finite regression moments");
        let row = &result.rows()[0];
        assert!(row.iter().all(|value| matches!(value, Value::Float64(value) if value.is_nan())));
    }
}

#[test]
fn should_propagate_later_nonfinite_regression_moments() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-nonfinite-later-row", &[]);

    // Act
    let result = fixture.execute(
        "SELECT kind, regr_sxx(y, x), regr_syy(y, x), regr_sxy(y, x) FROM (VALUES \
         ('x_infinity',1::FLOAT8,1::FLOAT8), \
         ('x_infinity','Infinity'::FLOAT8,2::FLOAT8), \
         ('y_nan',1::FLOAT8,1::FLOAT8), \
         ('y_nan',2::FLOAT8,'NaN'::FLOAT8)) AS pairs(kind,x,y) \
         GROUP BY kind ORDER BY kind",
    );

    // Assert
    let result = result.expect("later non-finite regression moments");
    let rows = result.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], Value::String("x_infinity".into()));
    assert!(matches!(rows[0][1], Value::Float64(value) if value.is_nan()));
    assert_eq!(rows[0][2], Value::Float64(0.5));
    assert!(matches!(rows[0][3], Value::Float64(value) if value.is_nan()));
    assert_eq!(rows[1][0], Value::String("y_nan".into()));
    assert_eq!(rows[1][1], Value::Float64(0.5));
    assert!(matches!(rows[1][2], Value::Float64(value) if value.is_nan()));
    assert!(matches!(rows[1][3], Value::Float64(value) if value.is_nan()));
}

#[test]
fn should_reject_distinct_regression_aggregates() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-distinct", &[]);

    // Act
    let result = fixture.execute(
        "SELECT corr(DISTINCT y, x) \
         FROM (VALUES (1::FLOAT8, 2::FLOAT8)) AS pairs(y, x)",
    );

    // Assert
    assert!(matches!(result, Err(cassie::app::CassieError::Unsupported(_))));
}

#[test]
fn should_reject_ordered_regression_aggregates() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-order-by", &[]);

    // Act
    let result = fixture.execute(
        "SELECT corr(y, x ORDER BY x) \
         FROM (VALUES (1::FLOAT8, 2::FLOAT8)) AS pairs(y, x)",
    );

    // Assert
    assert!(matches!(result, Err(cassie::app::CassieError::Unsupported(_))));
}

#[test]
fn should_reject_regression_window_aggregates() {
    // Arrange
    let fixture = sql_fixture("issue-806-regression-window", &[]);

    // Act
    let result = fixture.execute(
        "SELECT corr(y, x) OVER (ORDER BY x) \
         FROM (VALUES (1::FLOAT8, 2::FLOAT8), (2::FLOAT8, 3::FLOAT8)) AS pairs(y, x)",
    );

    // Assert
    assert!(matches!(result, Err(cassie::app::CassieError::Unsupported(_))));
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

#[test]
fn should_preserve_statistical_result_family_from_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-input-result-families",
        &[
            "CREATE TABLE issue_806_statistical_types (r REAL, n NUMERIC)",
            "INSERT INTO issue_806_statistical_types VALUES (1,1),(3,3),(5,5)",
        ],
    );

    // Act
    let real = fixture.execute(
        "SELECT var_pop(r), stddev_samp(r) FROM issue_806_statistical_types",
    );
    let numeric = fixture.execute(
        "SELECT var_pop(n), stddev_samp(n) FROM issue_806_statistical_types",
    );

    // Assert
    let real = real.expect("REAL statistics return double precision");
    assert_float_near(&real.rows()[0][0], 8.0 / 3.0);
    assert_float_near(&real.rows()[0][1], 2.0);
    assert_eq!(
        real.columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701]
    );

    let numeric = numeric.expect("NUMERIC statistics retain numeric identity");
    let expected_variance =
        numeric_math::divide(&Numeric::from_i128(8), &Numeric::from_i128(3))
            .expect("numeric population variance");
    assert_eq!(
        numeric.rows()[0],
        [
            Value::String(expected_variance.to_string()),
            Value::String("2.0000000000000000".to_owned()),
        ]
    );
    assert_eq!(
        numeric
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700]
    );
}

#[test]
fn should_keep_numeric_variance_exact_for_large_shared_offsets() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-large-offset-numeric-variance",
        &[
            "CREATE TABLE issue_806_large_offset_numeric (n NUMERIC)",
            "INSERT INTO issue_806_large_offset_numeric VALUES \
             (123456789012345678901234567890.123456789), \
             (123456789012345678901234567890.123456790), \
             (123456789012345678901234567890.123456791)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_samp(n) FROM issue_806_large_offset_numeric",
    );

    // Assert
    let result = result.expect("exact moments should survive a large shared NUMERIC offset");
    let expected_population = numeric_math::divide(
        &parse_text("0.000000000000000002").expect("population variance numerator"),
        &Numeric::from_i128(3),
    )
    .expect("population variance division");
    let expected_sample = parse_text("0.000000000000000001").expect("sample variance");
    let expected_stddev = parse_text("0.000000001").expect("sample standard deviation");
    let expected_values = [expected_population, expected_sample, expected_stddev];
    for (actual, expected) in result.rows()[0].iter().zip(expected_values) {
        let Value::String(actual) = actual else {
            panic!("NUMERIC result should retain text carrier, got {actual:?}");
        };
        let actual = parse_text(actual).expect("parse NUMERIC result");
        assert_eq!(actual.compare(&expected), std::cmp::Ordering::Equal);
    }
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700; 3]
    );
}

#[test]
fn should_return_numeric_nan_for_nonfinite_statistical_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-806-nonfinite-numeric-variance", &[]);
    let inputs = ["NaN", "Infinity", "-Infinity"];

    // Act
    let results = inputs.map(|input| {
        fixture
            .execute(&format!(
                "SELECT VAR_POP('{input}'::NUMERIC), STDDEV_POP('{input}'::NUMERIC)"
            ))
            .expect("nonfinite NUMERIC statistical aggregates")
    });

    // Assert
    for result in results {
        assert_eq!(
            result.rows(),
            &[vec![
                Value::String("NaN".to_owned()),
                Value::String("NaN".to_owned()),
            ]]
        );
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![1700; 2]
        );
    }
}

#[test]
fn should_preserve_numeric_result_family_when_nonfinite_and_finite_rows_mix() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-mixed-nonfinite-numeric-statistics",
        &[
            "CREATE TABLE issue_806_mixed_nonfinite_numeric (value NUMERIC)",
            "INSERT INTO issue_806_mixed_nonfinite_numeric VALUES ('NaN'), (1)",
            "CREATE TABLE issue_806_mixed_nonfinite_numeric_reverse (value NUMERIC)",
            "INSERT INTO issue_806_mixed_nonfinite_numeric_reverse VALUES (1), ('NaN')",
        ],
    );

    // Act
    let results = [
        fixture.execute(
            "SELECT VAR_POP(value), STDDEV_POP(value) FROM issue_806_mixed_nonfinite_numeric",
        ),
        fixture.execute(
            "SELECT VAR_POP(value), STDDEV_POP(value) FROM issue_806_mixed_nonfinite_numeric_reverse",
        ),
    ];

    // Assert
    for result in results {
        let result = result.expect("mixed finite and nonfinite NUMERIC statistics");
        assert_eq!(
            result.rows(),
            &[vec![
                Value::String("NaN".to_owned()),
                Value::String("NaN".to_owned()),
            ]]
        );
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![1700, 1700]
        );
    }
}

#[test]
fn should_preserve_statistical_states_per_group() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-grouped-statistical-states",
        &[
            "CREATE TABLE issue_806_grouped_statistics \
             (label TEXT, exact_n NUMERIC, approximate_n FLOAT8)",
            "INSERT INTO issue_806_grouped_statistics \
             VALUES ('a',1,1),('a',3,3),('b',10,10),('b',14,14)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT label, VAR_POP(exact_n), STDDEV_SAMP(exact_n), \
         VAR_POP(approximate_n), STDDEV_SAMP(approximate_n) \
         FROM issue_806_grouped_statistics GROUP BY label ORDER BY label",
    );

    // Assert
    let result = result.expect("grouped statistical aggregates retain independent states");
    assert_eq!(result.rows().len(), 2);
    let expected_numeric = [
        (
            "a",
            Numeric::from_i128(1),
            numeric_math::sqrt(&Numeric::from_i128(2)).expect("group a sample stddev"),
            1.0,
            2.0_f64.sqrt(),
        ),
        (
            "b",
            Numeric::from_i128(4),
            numeric_math::sqrt(&Numeric::from_i128(8)).expect("group b sample stddev"),
            4.0,
            8.0_f64.sqrt(),
        ),
    ];
    for (row, (label, population, sample_deviation, float_population, float_deviation)) in
        result.rows().iter().zip(expected_numeric)
    {
        assert_eq!(row[0], Value::String(label.to_owned()));
        let Value::String(actual_population) = &row[1] else {
            panic!("NUMERIC group result should use text carrier, got {:?}", row[1]);
        };
        let actual_population = parse_text(actual_population).expect("parse group variance");
        assert_eq!(
            actual_population.compare(&population),
            std::cmp::Ordering::Equal
        );
        let Value::String(actual_deviation) = &row[2] else {
            panic!("NUMERIC group result should use text carrier, got {:?}", row[2]);
        };
        let actual_deviation = parse_text(actual_deviation).expect("parse group stddev");
        assert!(
            (actual_deviation.to_f64() - sample_deviation.to_f64()).abs() < 1e-15,
            "group {label}: actual {actual_deviation}, expected {sample_deviation}"
        );
        assert_eq!(row[3], Value::Float64(float_population));
        assert_eq!(row[4], Value::Float64(float_deviation));
    }
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 1700, 1700, 701, 701]
    );
}
