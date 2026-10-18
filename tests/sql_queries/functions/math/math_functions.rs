#[test]
fn should_retain_float8_type_when_rounding() {
    // Arrange
    let sql = "SELECT ROUND(CAST(42.4 AS FLOAT)), ROUND(CAST(-42.8 AS FLOAT))";

    // Act
    let fixture = sql_fixture("issue-813-round-float8", &[]);
    let result = fixture.execute(sql);

    // Assert
    let result = result.expect("float8 rounding");
    assert_eq!(
        result.rows(),
        &[vec![Value::Float64(42.0), Value::Float64(-43.0)]]
    );
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
fn should_round_numeric_ties_away_from_zero_at_the_requested_scale() {
    // Arrange
    let sql = "SELECT ROUND(CAST('1.245' AS NUMERIC), 2) = CAST('1.25' AS NUMERIC) AND ROUND(CAST('-1.245' AS NUMERIC), 2) = CAST('-1.25' AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-round-numeric-ties", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_round_numeric_values_at_negative_scales() {
    // Arrange
    let sql = "SELECT ROUND(CAST(1234.56 AS NUMERIC), -1) = CAST(1230 AS NUMERIC) AND ROUND(CAST(-1234.56 AS NUMERIC), -1) = CAST(-1230 AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-round-negative-scale", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_truncate_numeric_values_toward_zero_at_signed_scales() {
    // Arrange
    let sql = "SELECT TRUNC(CAST('42.4382' AS NUMERIC), 2) = CAST('42.43' AS NUMERIC) AND TRUNC(CAST('-125.9' AS NUMERIC), -1) = CAST('-120' AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-trunc-numeric-scales", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_floor_float8_values_on_both_sides_of_zero() {
    // Arrange
    let sql = "SELECT FLOOR(CAST(42.8 AS FLOAT)) = 42.0 AND FLOOR(CAST(-42.8 AS FLOAT)) = -43.0";

    // Act
    let rows = predicate_rows("issue-813-floor-float8", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_ceil_float8_values_on_both_sides_of_zero() {
    // Arrange
    let sql = "SELECT CEIL(CAST(42.2 AS FLOAT)) = 43.0 AND CEIL(CAST(-42.8 AS FLOAT)) = -42.0";

    // Act
    let rows = predicate_rows("issue-813-ceil-float8", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_preserve_numeric_types_for_floor_ceil() {
    // Arrange
    let sql = "SELECT FLOOR(CAST(42.8 AS NUMERIC)) = CAST(42 AS NUMERIC) AND CEIL(CAST(-42.8 AS NUMERIC)) = CAST(-42 AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-floor-ceil-numeric", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_support_ceiling_as_the_ceil_alias() {
    // Arrange
    let sql = "SELECT CEILING(CAST(95.3 AS FLOAT)) = CEIL(CAST(95.3 AS FLOAT))";

    // Act
    let rows = predicate_rows("issue-813-ceiling-alias", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_propagate_null_through_rounding_functions() {
    // Arrange
    let sql = "SELECT ROUND(CAST(NULL AS FLOAT)) IS NULL AND TRUNC(CAST(NULL AS FLOAT)) IS NULL AND FLOOR(CAST(NULL AS FLOAT)) IS NULL AND CEILING(CAST(NULL AS FLOAT)) IS NULL";

    // Act
    let rows = predicate_rows("issue-813-rounding-null", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_numeric_modulo_with_the_dividend_sign() {
    // Arrange
    let sql = "SELECT MOD(9, 4) = 1 AND MOD(-9, 4) = -1 AND MOD(9, -4) = 1";

    // Act
    let rows = predicate_rows("issue-813-mod-sign", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_divide_numeric_values_by_truncating_toward_zero() {
    // Arrange
    let sql = "SELECT DIV(9, 4) = 2 AND DIV(-9, 4) = -2 AND DIV(9, -4) = -2";

    // Act
    let rows = predicate_rows("issue-813-div-truncation", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_preserve_exact_numeric_modulo_quotient_values() {
    // Arrange
    let sql = "SELECT MOD(CAST(-9.5 AS NUMERIC), CAST(4 AS NUMERIC)) = CAST(-1.5 AS NUMERIC) AND DIV(CAST(-9.5 AS NUMERIC), CAST(4 AS NUMERIC)) = CAST(-2 AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-mod-div-numeric", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_keep_power_result_types_for_both_inputs() {
    // Arrange
    let sql = "SELECT POWER(CAST(9 AS NUMERIC), CAST(3 AS NUMERIC)) = CAST(729 AS NUMERIC) AND POWER(CAST(9 AS FLOAT), CAST(0.5 AS FLOAT)) = CAST(3 AS FLOAT)";

    // Act
    let rows = predicate_rows("issue-813-power-overloads", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_apply_the_power_operator_left_to_right() {
    // Arrange
    let sql = "SELECT 2 ^ 3 ^ 3 AS power";

    // Act
    let fixture = sql_fixture("issue-813-power-operator", &[]);
    let result = fixture.execute(sql);

    // Assert
    let result = result.expect("left-associative PostgreSQL power operator");
    assert_eq!(result.rows(), &[vec![Value::Float64(512.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_calculate_the_prefix_square_root_operator() {
    // Arrange
    let fixture = sql_fixture("issue-813-square-root-operator", &[]);

    // Act
    let result = fixture.execute("SELECT |/ 25.0 AS square_root");

    // Assert
    let result = result.expect("PostgreSQL prefix square-root operator");
    assert_eq!(result.rows(), &[vec![Value::Float64(5.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_calculate_square_roots_for_each_numeric_type() {
    // Arrange
    let sql = "SELECT SQRT(CAST(4 AS NUMERIC)) = CAST(2 AS NUMERIC) AND SQRT(CAST(25 AS FLOAT)) = CAST(5 AS FLOAT)";

    // Act
    let rows = predicate_rows("issue-813-sqrt-overloads", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_evaluate_exponential_logarithm_functions() {
    // Arrange
    let sql = "SELECT EXP(CAST(1 AS FLOAT)), LN(CAST(1 AS FLOAT)), LOG(CAST(1000 AS FLOAT)), LOG10(CAST(1000 AS FLOAT))";

    // Act
    let values = scalar_values("issue-813-log-family", sql);

    // Assert
    assert_float_values(values, &[E, 0.0, 3.0, 3.0]);
}

#[test]
fn should_calculate_logarithms_with_an_explicit_base() {
    // Arrange
    let sql = "SELECT LOG(CAST(2 AS NUMERIC), CAST(64 AS NUMERIC)) = CAST(6 AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-log-base", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_sign_values_for_signed_float8_inputs() {
    // Arrange
    let sql =
        "SELECT SIGN(CAST(-8.4 AS FLOAT)), SIGN(CAST(0.0 AS FLOAT)), SIGN(CAST(-0.0 AS FLOAT))";

    // Act
    let values = scalar_values("issue-813-sign-signed-zero", sql);

    // Assert
    assert_float_values(values, &[-1.0, 0.0, 0.0]);
}

#[test]
fn should_return_pi_as_a_float8_value() {
    // Arrange
    let sql = "SELECT PI() > 3.14 AND PI() < 3.15";

    // Act
    let rows = predicate_rows("issue-813-pi", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_calculate_gcd_for_integer_values_including_zero_inputs() {
    // Arrange
    let sql = "SELECT GCD(1071, 462) = 21 AND GCD(0, 0) = 0";

    // Act
    let rows = predicate_rows("issue-813-gcd", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_calculate_lcm_for_integer_values_including_zero_inputs() {
    // Arrange
    let sql = "SELECT LCM(1071, 462), LCM(0, 9)";

    // Act
    let values = scalar_values("issue-813-lcm", sql);

    // Assert
    assert_eq!(values, vec![Value::Int64(23_562), Value::Int64(0)]);
}

#[test]
fn should_return_factorial_as_an_exact_numeric_value() {
    // Arrange
    let sql = "SELECT FACTORIAL(21) = CAST('51090942171709440000' AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-factorial-exact", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_assign_width_buckets_with_both_bound_orders() {
    // Arrange
    let sql = "SELECT WIDTH_BUCKET(5.35, 0.024, 10.06, 5) = 3 AND WIDTH_BUCKET(0.023, 0.024, 10.06, 5) = 0 AND WIDTH_BUCKET(10.06, 0.024, 10.06, 5) = 6 AND WIDTH_BUCKET(9, 10, 0, 10) = 2";

    // Act
    let rows = predicate_rows("issue-813-width-bucket", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_assign_width_bucket_from_sorted_array_thresholds() {
    // Arrange
    let sql = "SELECT WIDTH_BUCKET(2, ARRAY[1, 3, 7]) = 1 AND WIDTH_BUCKET(0, ARRAY[1, 3, 7]) = 0 AND WIDTH_BUCKET(7, ARRAY[1, 3, 7]) = 3";

    // Act
    let rows = predicate_rows("issue-813-width-bucket-array", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_numeric_scale_metrics() {
    // Arrange
    let sql = "SELECT SCALE(CAST(8.4100 AS NUMERIC)) = 4 AND MIN_SCALE(CAST(8.4100 AS NUMERIC)) = 2 AND TRIM_SCALE(CAST(8.4100 AS NUMERIC)) = CAST(8.41 AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-numeric-scale-functions", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_convert_angle_units() {
    // Arrange
    let sql = "SELECT DEGREES(3.141592653589793), RADIANS(180.0)";

    // Act
    let values = scalar_values("issue-813-angle-conversions", sql);

    // Assert
    assert_float_values(values, &[180.0, PI]);
}

#[test]
fn should_evaluate_radian_trigonometric_functions() {
    // Arrange
    let sql = "SELECT SIN(1.0), COS(0.0), TAN(1.0), COT(0.5), ACOS(1.0), ASIN(1.0), ATAN(1.0), ATAN2(1.0, 0.0)";

    // Act
    let values = scalar_values("issue-813-trigonometric-radians", sql);

    // Assert
    assert_float_values(
        values,
        &[
            0.841_470_984_807_896_5,
            1.0,
            1.557_407_724_654_902_3,
            1.830_487_721_712_452,
            0.0,
            FRAC_PI_2,
            FRAC_PI_4,
            FRAC_PI_2,
        ],
    );
}

#[test]
fn should_evaluate_degree_trigonometric_functions() {
    // Arrange
    let sql = "SELECT SIND(30.0), COSD(60.0), TAND(45.0), COTD(45.0), ASIND(0.5), ACOSD(0.5), ATAND(1.0), ATAN2D(1.0, 0.0)";

    // Act
    let values = scalar_values("issue-813-trigonometric-degrees", sql);

    // Assert
    assert_float_values(values, &[0.5, 0.5, 1.0, 1.0, 30.0, 60.0, 45.0, 90.0]);
}

#[test]
fn should_evaluate_hyperbolic_functions() {
    // Arrange
    let sql = "SELECT SINH(1.0), COSH(0.0), TANH(1.0), ASINH(1.0), ACOSH(1.0), ATANH(0.5)";

    // Act
    let values = scalar_values("issue-813-hyperbolic-functions", sql);

    // Assert
    assert_float_values(
        values,
        &[
            1.175_201_193_643_801_4,
            1.0,
            0.761_594_155_955_764_9,
            0.881_373_587_019_543,
            0.0,
            0.549_306_144_334_054_8,
        ],
    );
}

#[test]
fn should_evaluate_erf_pair() {
    // Arrange
    let sql = "SELECT ERF(1.0), ERFC(1.0)";

    // Act
    let values = scalar_values("issue-813-error-functions", sql);

    // Assert
    assert_float_values(values, &[0.842_700_792_949_714_9, 0.157_299_207_050_285_13]);
}

#[test]
fn should_evaluate_gamma_functions() {
    // Arrange
    let sql = "SELECT GAMMA(6.0), LGAMMA(1.0)";

    // Act
    let values = scalar_values("issue-813-gamma-functions", sql);

    // Assert
    assert_float_values(values, &[120.0, 0.0]);
}

#[test]
fn should_return_random_float8_values_in_the_half_open_unit_interval() {
    // Arrange
    let sql = "SELECT RANDOM()";

    // Act
    let values = scalar_values("issue-813-random-float8", sql);

    // Assert
    let [Value::Float64(value)] = values.as_slice() else {
        panic!("random() should return one float8 value: {values:?}");
    };
    assert!((0.0..1.0).contains(value), "random() returned {value}");
}

#[test]
fn should_return_random_integer_values_inside_the_inclusive_bounds() {
    // Arrange
    let sql = "SELECT RANDOM(1, 10)";

    // Act
    let values = scalar_values("issue-813-random-integer-bounds", sql);

    // Assert
    let [Value::Int64(value)] = values.as_slice() else {
        panic!("random(integer, integer) should return int4: {values:?}");
    };
    assert!((1..=10).contains(value), "random(1, 10) returned {value}");
}

#[test]
fn should_return_random_bigint_values_inside_the_inclusive_bounds() {
    // Arrange
    let sql = "SELECT RANDOM(CAST(9007199254740992 AS BIGINT), CAST(9007199254740994 AS BIGINT))";

    // Act
    let values = scalar_values("issue-813-random-bigint-bounds", sql);

    // Assert
    let [Value::Int64(value)] = values.as_slice() else {
        panic!("random(bigint, bigint) should return int8: {values:?}");
    };
    assert!(
        (9_007_199_254_740_992..=9_007_199_254_740_994).contains(value),
        "random(bigint bounds) returned {value}"
    );
}

#[test]
fn should_return_random_numeric_values_with_the_maximum_bound_scale() {
    // Arrange
    let sql = "SELECT RANDOM(CAST(1.0 AS NUMERIC), CAST(2.00 AS NUMERIC)) BETWEEN CAST(1.00 AS NUMERIC) AND CAST(2.00 AS NUMERIC)";

    // Act
    let rows = predicate_rows("issue-813-random-numeric-bounds", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_support_random_normal_arities() {
    // Arrange
    let sql = "SELECT RANDOM_NORMAL(), RANDOM_NORMAL(0.0), RANDOM_NORMAL(0.0, 1.0)";

    // Act
    let values = scalar_values("issue-813-random-normal-arity", sql);

    // Assert
    assert_eq!(values.len(), 3);
    assert!(values
        .iter()
        .all(|value| matches!(value, Value::Float64(_))));
}

#[test]
fn should_repeat_random_sequences_after_reseeding_the_session() {
    // Arrange
    let fixture = sql_fixture("issue-813-seeded-random-sequence", &[]);
    fixture
        .execute("SELECT SETSEED(0.12345)")
        .expect("seed random sequence");

    // Act
    let first = fixture
        .execute("SELECT RANDOM(), RANDOM_NORMAL()")
        .expect("first seeded sequence")
        .rows()
        .to_vec();
    fixture
        .execute("SELECT SETSEED(0.12345)")
        .expect("reset random sequence");
    let repeated = fixture
        .execute("SELECT RANDOM(), RANDOM_NORMAL()")
        .expect("repeated seeded sequence")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(first, repeated);
}
