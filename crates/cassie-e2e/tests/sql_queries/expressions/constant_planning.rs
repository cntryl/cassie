//! PostgreSQL folds immutable constant subexpressions while planning, so a
//! constant `1/0` fails even in a dynamically dead CASE branch or a query that
//! reads no rows. Row-dependent expressions stay lazy, and CASE, AND/OR and
//! COALESCE prune constant inputs in argument order exactly as the folding does.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::executor::QueryResult;
use cassie::types::Value;

fn planning_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE cp (id BIGINT)",
            "INSERT INTO cp VALUES (1), (2)",
            "CREATE TABLE cp_empty (id BIGINT)",
        ],
    )
}

fn is_division_by_zero(result: &Result<QueryResult, CassieError>) -> bool {
    matches!(result, Err(CassieError::Execution(message)) if message == "division by zero")
}

#[test]
fn should_raise_constant_errors_before_reading_rows() {
    // Arrange
    let fixture = planning_fixture("constant_planning_select_errors");
    let cases = [
        "SELECT id, CASE WHEN id < 0 THEN 1/0 ELSE 1 END AS selected FROM cp",
        "SELECT 1/0 AS selected FROM cp_empty",
        "SELECT 1/0 AS selected FROM cp WHERE id < 0",
        "SELECT id FROM cp WHERE id < 0 AND 1/0 = 1",
        "SELECT COALESCE(id, 1/0) AS selected FROM cp",
        "SELECT CASE WHEN id < 0 THEN 1 END + 2/0 AS selected FROM cp_empty",
        "SELECT id FROM cp_empty ORDER BY id + 1/0",
        "SELECT 1 AS selected FROM cp WHERE id < 0 AND EXISTS(SELECT 1 FROM cp_empty WHERE 1/0 = 1)",
        "WITH c AS (SELECT 1/0 AS x FROM cp_empty) SELECT x FROM c",
        "SELECT id FROM cp_empty UNION ALL SELECT 1/0 FROM cp_empty",
        "SELECT sum(1/0) AS selected FROM cp_empty",
        "SELECT 1 IN (1, 1/0) AS selected FROM cp_empty",
        "SELECT 5 NOT BETWEEN 1 AND 1/0 AS selected FROM cp_empty",
        "SELECT CASE WHEN concat('a', 'b') = 'ab' THEN 1 ELSE 1/0 END AS selected FROM cp_empty",
    ];

    // Act
    let results = cases.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in cases.iter().zip(&results) {
        assert!(is_division_by_zero(result), "planning-time 22012: {sql}");
    }
}

#[test]
fn should_keep_unfolded_expressions_lazy() {
    // Arrange
    let fixture = planning_fixture("constant_planning_select_lazy");
    let cases = [
        (
            "SELECT CASE WHEN id < 0 THEN cp.id/0 ELSE 1 END AS selected FROM cp ORDER BY id",
            vec![vec![Value::Int64(1)], vec![Value::Int64(1)]],
        ),
        ("SELECT cp_empty.id/0 AS selected FROM cp_empty", Vec::new()),
        (
            "SELECT CASE WHEN true THEN 1 ELSE 1/0 END AS selected FROM cp_empty",
            Vec::new(),
        ),
        (
            "SELECT CASE WHEN false THEN 1/0 ELSE 2 END AS selected FROM cp WHERE id = 1",
            vec![vec![Value::Int64(2)]],
        ),
        (
            "SELECT CASE WHEN NULL THEN 1/0 ELSE 3 END AS selected FROM cp WHERE id = 1",
            vec![vec![Value::Int64(3)]],
        ),
        (
            "SELECT CASE 1 WHEN 2 THEN 1/0 ELSE 4 END AS selected FROM cp WHERE id = 1",
            vec![vec![Value::Int64(4)]],
        ),
        (
            "SELECT COALESCE(5, 1/0) AS selected FROM cp WHERE id = 1",
            vec![vec![Value::Int64(5)]],
        ),
        (
            "SELECT id FROM cp_empty WHERE false AND 1/0 = 1",
            Vec::new(),
        ),
        (
            "SELECT id FROM cp_empty WHERE id < 0 AND (true OR 1/0 = 1)",
            Vec::new(),
        ),
        ("SELECT 1/NULL AS selected FROM cp_empty", Vec::new()),
        (
            "WITH c AS (SELECT 1/0 AS x FROM cp_empty) SELECT id FROM cp_empty",
            Vec::new(),
        ),
        (
            "SELECT CASE WHEN length('abc') > 5 THEN 1/0 ELSE 1 END AS selected FROM cp_empty",
            Vec::new(),
        ),
        (
            "SELECT CASE WHEN abs(-3) > 5 THEN 1/0 ELSE 1 END AS selected FROM cp_empty",
            Vec::new(),
        ),
        (
            "SELECT CASE WHEN (false AND id > 0) IS NULL THEN 1/0 ELSE 1 END AS selected FROM cp_empty",
            Vec::new(),
        ),
        (
            "SELECT 5 BETWEEN 10 AND 1/0 AS selected FROM cp_empty",
            Vec::new(),
        ),
        (
            "SELECT 5 NOT BETWEEN 10 AND 1/0 AS selected FROM cp_empty",
            Vec::new(),
        ),
    ];

    // Act
    let results: Vec<Vec<Vec<Value>>> = cases.iter().map(|(sql, _)| fixture.rows(sql)).collect();

    // Assert
    for ((sql, expected), actual) in cases.iter().zip(&results) {
        assert_eq!(actual, expected, "lazy or pruned: {sql}");
    }
}

#[test]
fn should_keep_parameter_dependent_division_at_runtime() {
    // Arrange
    let fixture = planning_fixture("constant_planning_parameters");

    // Act
    let reached = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT 1/$1 AS selected FROM cp",
        vec![Value::Int64(0)],
    );
    let divided = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT 6/$1 AS selected FROM cp WHERE id = 1",
        vec![Value::Int64(3)],
    );

    // Assert
    assert!(is_division_by_zero(&reached), "reached parameter division");
    assert_eq!(
        divided.map(|result| result.rows().to_vec()).ok(),
        Some(vec![vec![Value::Int64(2)]])
    );
}

#[test]
fn should_propagate_folded_float_power_range_with_embedded_type_input() {
    // Arrange
    let fixture = planning_fixture("constant_planning_float_power_range");
    let cases = [
        ("SELECT power(1e308::float8, 2::float8) FROM cp_empty", "overflow"),
        ("SELECT 1e-300::float8 ^ 2::float8 FROM cp_empty", "underflow"),
        ("SELECT CASE WHEN id < 0 THEN power(1e308::float8, 2::float8) ELSE 1::float8 END FROM cp", "overflow"),
        ("SELECT COALESCE(id::float8, 1e-300::float8 ^ 2::float8) FROM cp", "underflow"),
    ];

    // Act
    for (sql, expected) in cases {
        let result = fixture.execute(sql);
        // Assert
        assert!(
            matches!(result, Err(CassieError::TypeInput(ref error))
            if error.sqlstate == "22003" && error.message == format!("value out of range: {expected}")),
            "{sql}: {result:?}"
        );
    }
}

#[test]
fn should_prune_unreached_float_power_range_constants() {
    // Arrange
    let fixture = planning_fixture("constant_planning_float_power_lazy");
    let cases = [
        ("SELECT CASE WHEN false THEN power(1e308::float8, 2::float8) ELSE 1::float8 END FROM cp WHERE id = 1", vec![vec![Value::Float64(1.0)]]),
        ("SELECT COALESCE(2::float8, 1e-300::float8 ^ 2::float8) FROM cp WHERE id = 1", vec![vec![Value::Float64(2.0)]]),
        ("SELECT id FROM cp WHERE false AND power(1e308::float8, 2::float8) = 1::float8", Vec::new()),
        ("SELECT id FROM cp WHERE true OR 1e-300::float8 ^ 2::float8 = 1::float8 ORDER BY id", vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]),
        ("SELECT CASE WHEN false THEN (SELECT power(1e308::float8, 2::float8)) ELSE 3::float8 END FROM cp WHERE id = 1", vec![vec![Value::Float64(3.0)]]),
    ];

    // Act
    for (sql, expected) in cases {
        // Assert
        assert_eq!(fixture.rows(sql), expected, "{sql}");
    }
}

#[test]
fn should_preserve_bound_subquery_power_range_diagnostics() {
    // Arrange
    let fixture = planning_fixture("constant_planning_float_power_bound_subquery");
    let cases = [
        (
            "SELECT power($1::float8, 2::float8) FROM cp_empty",
            vec![Value::Float64(1e308)],
            "overflow",
        ),
        (
            "SELECT (SELECT 1e-300::float8 ^ 2::float8) FROM cp WHERE id = 1",
            vec![],
            "underflow",
        ),
    ];

    // Act
    for (sql, params, expected) in cases {
        let result = fixture.cassie.execute_sql(&fixture.session, sql, params);
        // Assert
        assert!(
            matches!(result, Err(CassieError::TypeInput(ref error))
            if error.sqlstate == "22003" && error.message == format!("value out of range: {expected}")),
            "{sql}: {result:?}"
        );
    }
}

#[test]
fn should_defer_row_dependent_subquery_power_failure_until_its_branch_is_reached() {
    // Arrange
    let fixture = planning_fixture("constant_planning_float_power_subquery_runtime");
    let query = |outer_id| {
        format!("SELECT CASE WHEN cp.id = 1 THEN (SELECT power(inner_row.id::float8 * 1e-300::float8, 2::float8) FROM cp AS inner_row WHERE inner_row.id = 1) ELSE 3::float8 END FROM cp WHERE cp.id = {outer_id}")
    };

    // Act
    let unreached = fixture.execute(&query(2));
    let reached = fixture.execute(&query(1));

    // Assert
    assert_eq!(
        unreached.expect("unreached subquery").rows(),
        &[vec![Value::Float64(3.0)]]
    );
    assert!(
        matches!(reached, Err(CassieError::TypeInput(ref error)) if error.sqlstate == "22003" && error.message == "value out of range: underflow")
    );
}
