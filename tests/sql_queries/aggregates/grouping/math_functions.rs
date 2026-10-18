//! PostgreSQL 18 typed mathematical functions tracked by #813.

use cassie::types::Value;
use std::f64::consts::{E, FRAC_PI_2, FRAC_PI_4, PI};

use crate::support_sql_fixture::sql_fixture;

fn predicate_rows(label: &str, sql: &str) -> Vec<Vec<Value>> {
    let fixture = sql_fixture(label, &[]);
    fixture
        .execute(sql)
        .unwrap_or_else(|error| panic!("math query failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

fn scalar_values(label: &str, sql: &str) -> Vec<Value> {
    let fixture = sql_fixture(label, &[]);
    fixture
        .execute(sql)
        .unwrap_or_else(|error| panic!("math query failed: {sql}: {error}"))
        .rows()
        .first()
        .expect("math query returns one row")
        .clone()
}

fn assert_float_values(actual: Vec<Value>, expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (value, expected) in actual.into_iter().zip(expected) {
        let Value::Float64(value) = value else {
            panic!("expected float8 value, got {value:?}");
        };
        assert!(
            (value - expected).abs() <= 1e-12,
            "expected {expected}, got {value}"
        );
    }
}

#[test]
fn should_calculate_the_prefix_cube_root_operator_for_negative_float8() {
    // Arrange
    let fixture = sql_fixture("issue-813-cube-root-operator", &[]);

    // Act
    let result = fixture.execute("SELECT ||/ -64.0 AS cube_root");

    // Assert
    let result = result.expect("PostgreSQL prefix cube-root operator for negative float8");
    assert_eq!(result.rows(), &[vec![Value::Float64(-4.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_calculate_cube_roots_for_negative_float8_values() {
    // Arrange
    let sql = "SELECT CBRT(CAST(-8 AS FLOAT)) = CAST(-2 AS FLOAT)";

    // Act
    let rows = predicate_rows("issue-813-cbrt-negative", sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

include!("../../functions/math/math_functions.rs");
