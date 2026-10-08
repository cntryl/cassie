use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_boolean_for_every_null_safe_truth_table_pair() {
    // Arrange
    let fixture = sql_fixture("null_safe_truth_table", &[]);
    let cases = [
        ("NULL", "NULL", false),
        ("NULL", "7", true),
        ("7", "NULL", true),
        ("7", "7", false),
        ("7", "8", true),
    ];
    // Act
    for (left, right, distinct) in cases {
        let result = fixture.execute(&format!(
            "SELECT {left} IS DISTINCT FROM {right} AS different, {left} IS NOT DISTINCT FROM {right} AS same"
        )).expect("NULL-safe scalar evaluation");
        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Bool(distinct), Value::Bool(!distinct)]]
        );
        assert_eq!(result.columns.len(), 2);
        for column in &result.columns {
            assert_eq!(
                (column.type_oid, column.typlen, column.atttypmod),
                (16, 1, -1)
            );
        }
    }
}

#[test]
fn should_match_current_json_equality_for_contextual_literals() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_json_equality",
        &[
            "CREATE TABLE documents (j JSON)",
            "INSERT INTO documents VALUES ('{\"n\":1}')",
        ],
    );
    // Act
    let result = fixture.execute(
        "SELECT j = '{\"n\":1.0}' AS ordinary, j IS NOT DISTINCT FROM '{\"n\":1.0}' AS same, j IS DISTINCT FROM '{\"n\":2}' AS different FROM documents"
    ).expect("current JSON predicate equality");
    // Assert
    assert_eq!(
        result.rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_preserve_current_scalar_equality_at_null_safe_comparisons() {
    // Arrange
    let fixture = sql_fixture("null_safe_scalar_equality", &[]);
    let cases = [
        ("9007199254740993", "9007199254740992.0", true),
        ("-0.0", "0.0", false),
        ("true", "false", true),
        ("'same'", "'same'", false),
        (
            "CAST('2026-10-08T01:02:03Z' AS TIMESTAMP)",
            "CAST('2026-10-08T01:02:03.000000000Z' AS TIMESTAMP)",
            false,
        ),
    ];
    // Act
    for (left, right, distinct) in cases {
        let result = fixture
            .execute(&format!(
                "SELECT {left} IS DISTINCT FROM {right} AS different"
            ))
            .expect("selected current scalar family");
        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Bool(distinct)]],
            "{left} versus {right}"
        );
    }
}

#[test]
fn should_propagate_right_operand_errors_after_null() {
    // Arrange
    let fixture = sql_fixture("null_safe_right_error", &[]);
    // Act
    let division = fixture.execute("SELECT NULL IS DISTINCT FROM (1 / 0) AS different");
    let incompatible =
        fixture.execute("SELECT CAST(NULL AS BOOLEAN) IS NOT DISTINCT FROM 1 AS same");
    // Assert
    assert!(
        division.is_err(),
        "NULL must not suppress right division by zero"
    );
    assert!(
        incompatible.is_err(),
        "typed NULL must not suppress declared family incompatibility"
    );
}

#[test]
fn should_preserve_current_canonical_nan_equality() {
    // Arrange
    let fixture = sql_fixture("null_safe_nan_boundary", &[]);
    // Act
    let result = fixture.cassie.execute_sql(&fixture.session,
        "SELECT $1 = $1 AS ordinary,$1 IS NOT DISTINCT FROM $1 AS same,$1 IS DISTINCT FROM NULL AS nonnull",
        vec![Value::Float64(f64::NAN)]).expect("existing canonical numeric equality");
    // Assert
    assert_eq!(
        result.rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_preserve_contextual_json_boundary_semantics() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_json_boundaries",
        &[
            "CREATE TABLE json_boundaries(id BIGINT,j JSON)",
            "INSERT INTO json_boundaries VALUES(1,NULL),(2,'null'),(3,'{\"n\":1}')",
        ],
    );
    // Act
    let nulls = fixture
        .execute("SELECT id,j IS NOT DISTINCT FROM NULL AS same FROM json_boundaries ORDER BY id")
        .expect("SQL NULL differs from JSON null");
    let malformed = ["=", "IS DISTINCT FROM", "IS NOT DISTINCT FROM"].map(|op| {
        [1, 3].map(|id| {
            fixture.execute(&format!(
                "SELECT j {op} 'invalid-json' AS result FROM json_boundaries WHERE id={id}"
            ))
        })
    });
    // Assert
    assert_eq!(
        nulls.rows,
        vec![
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(false)],
            vec![Value::Int64(3), Value::Bool(false)]
        ]
    );
    for result in malformed.into_iter().flatten() {
        assert!(
            matches!(result, Err(cassie::app::CassieError::Planner(ref message)) if message.contains("invalid JSON literal")),
            "{result:?}"
        );
    }
}
