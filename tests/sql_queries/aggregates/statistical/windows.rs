//! Finite PostgreSQL 18.6 statistics over the existing frame selector.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::{typed::TypedValue, Value};

const STATISTICS: [&str; 6] = [
    "var_pop",
    "var_samp",
    "variance",
    "stddev_pop",
    "stddev_samp",
    "stddev",
];
const REGRESSION: [&str; 12] = [
    "corr",
    "covar_pop",
    "covar_samp",
    "regr_count",
    "regr_slope",
    "regr_intercept",
    "regr_r2",
    "regr_sxx",
    "regr_sxy",
    "regr_syy",
    "regr_avgx",
    "regr_avgy",
];

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Null => None,
        Value::Float64(value) => Some(*value),
        Value::Typed(TypedValue::Numeric(value)) => Some(value.to_f64()),
        other => panic!("numeric result: {other:?}"),
    }
}

#[test]
fn should_filter_each_statistical_window_family_before_its_frame_transition() {
    // Arrange
    let fixture = sql_fixture("statistical-window-filter", &[]);
    let source = "(VALUES (1,1,TRUE),(2,9,FALSE),(3,3,TRUE),(4,NULL::INT,TRUE),(5,5,NULL::BOOLEAN))v(id,n,keep)";

    for input_type in ["INT", "NUMERIC", "REAL", "FLOAT8"] {
        for (index, name) in STATISTICS.into_iter().enumerate() {
            let sql = format!("SELECT pg_catalog.{name}(n::{input_type}) FILTER(WHERE keep) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND CURRENT ROW) FROM {source} ORDER BY id");
            let sample = matches!(index, 1 | 2 | 4 | 5);
            let middle = if index >= 4 {
                2_f64.sqrt()
            } else if sample {
                2.0
            } else {
                1.0
            };
            let expected = [
                if sample { None } else { Some(0.0) },
                if sample { None } else { Some(0.0) },
                Some(middle),
                if sample { None } else { Some(0.0) },
                if sample { None } else { Some(0.0) },
            ];

            // Act
            let result = fixture.execute(&sql).expect("filtered statistical frame");

            // Assert
            assert_eq!(result.row_count(), expected.len(), "{sql}");
            assert_eq!(
                result.columns()[0].type_oid,
                if matches!(input_type, "INT" | "NUMERIC") {
                    1700
                } else {
                    701
                },
                "{sql}"
            );
            for (row, expected) in result.rows().iter().zip(expected) {
                match (number(&row[0]), expected) {
                    (Some(actual), Some(expected)) => assert!(
                        (actual - expected).abs() < 1e-12,
                        "{sql}: {actual} != {expected}"
                    ),
                    (None, None) => {}
                    other => panic!("{sql}: {other:?}"),
                }
            }
        }
    }
}

#[test]
fn should_fold_all_regression_windows_with_pairwise_null_filtering() {
    // Arrange
    let fixture = sql_fixture("paired-regression-windows", &[]);
    let source = "(VALUES (1,2::FLOAT8,1::FLOAT8,TRUE),(2,99,2,FALSE),(3,6,3,TRUE),(4,NULL,4,TRUE),(5,10,NULL,TRUE))v(id,y,x,keep)";
    let expected = [
        Some(1.0),
        Some(2.0),
        Some(4.0),
        Some(2.0),
        Some(2.0),
        Some(0.0),
        Some(1.0),
        Some(2.0),
        Some(4.0),
        Some(8.0),
        Some(2.0),
        Some(4.0),
    ];
    let calls = REGRESSION.map(|name| format!("pg_catalog.{name}(y,x) FILTER(WHERE keep) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)")).join(",");
    let sql = format!("SELECT {calls} FROM {source} ORDER BY id");

    // Act
    let result = fixture.execute(&sql).expect("paired window frames");

    // Assert
    assert_eq!(result.row_count(), 5);
    for row in &result.rows()[2..] {
        for (index, expected) in expected.into_iter().enumerate() {
            if index == 3 {
                assert_eq!(row[index], Value::Int64(2));
            } else {
                assert_eq!(number(&row[index]), expected, "{}", REGRESSION[index]);
            }
        }
    }
    assert_eq!(result.rows()[0][3], Value::Int64(1));
    assert_eq!(result.rows()[0][0], Value::Null);
    for (index, column) in result.columns().iter().enumerate() {
        assert_eq!(column.type_oid, if index == 3 { 20 } else { 701 });
    }
}

#[test]
fn should_skip_both_window_arguments_given_false_or_null_filters() {
    // Arrange
    let fixture = sql_fixture("statistical-window-filter-order", &[]);

    // Act
    let result = fixture.execute("SELECT var_pop(1/(n-n)) FILTER(WHERE keep) OVER (), corr(1/(n-n),1/(n-n)) FILTER(WHERE keep) OVER () FROM (VALUES(1,FALSE),(2,NULL::BOOLEAN))v(n,keep)").expect("unselected arguments");

    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Null, Value::Null],
            vec![Value::Null, Value::Null]
        ]
    );
}

#[test]
fn should_return_zero_regression_count_given_an_empty_following_frame() {
    // Arrange
    let fixture = sql_fixture("regression-empty-frame", &[]);

    // Act
    let result = fixture.execute("SELECT regr_count(y,x) OVER w,regr_slope(y,x) OVER w FROM (VALUES(1,2::FLOAT8,1::FLOAT8),(2,6,3))v(id,y,x) WINDOW w AS(ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING) ORDER BY id").expect("following frames");

    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(0), Value::Null]
        ]
    );
}

#[test]
fn should_apply_exclude_ties_before_statistical_window_filters() {
    // Arrange
    let fixture = sql_fixture("statistical-exclude-ties", &[]);

    // Act
    let result = fixture.execute("SELECT var_pop(n) FILTER(WHERE keep) OVER w,covar_pop(n,n) FILTER(WHERE keep) OVER w FROM (VALUES(1,1,1::FLOAT8,TRUE),(2,1,3,TRUE),(3,2,5,FALSE))v(id,k,n,keep) WINDOW w AS(ORDER BY k ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW EXCLUDE TIES) ORDER BY id").expect("excluded peer frames");

    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Float64(0.0), Value::Float64(0.0)],
            vec![Value::Float64(0.0), Value::Float64(0.0)],
            vec![Value::Float64(1.0), Value::Float64(1.0)]
        ]
    );
}

#[test]
fn should_preserve_nonfinite_regression_results_across_moving_frames() {
    // Arrange
    let fixture = sql_fixture("nonfinite-regression-frame", &[]);

    // Act
    let result = fixture.execute("SELECT corr(y,x) OVER w,regr_sxx(y,x) OVER w FROM (VALUES(1,2::FLOAT8,1::FLOAT8),(2,'Infinity'::FLOAT8,3),(3,6,5))v(id,y,x) WINDOW w AS(ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) ORDER BY id").expect("nonfinite moving windows");

    // Assert
    assert_eq!(result.rows()[0], vec![Value::Null, Value::Float64(0.0)]);
    for row in &result.rows()[1..] {
        assert!(number(&row[0]).expect("NaN correlation").is_nan());
        assert_eq!(row[1], Value::Float64(2.0));
    }
}
