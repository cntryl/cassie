//! PostgreSQL 18 SQL-defined aggregate behavior tracked by #910.

#[path = "sql_support_functions.rs"]
mod sql_support_functions;

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_execute_sql_defined_aggregate_over_rows_in_a_table_expression() {
    // Arrange
    let fixture = sql_fixture("issue-910-sql-aggregate", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE AGGREGATE issue_910_sum(INTEGER) (SFUNC = pg_catalog.int4pl, STYPE = INTEGER, INITCOND = '0')",
        )
        .expect("CREATE AGGREGATE should accept a valid integer transition function");
    let sql = "SELECT issue_910_sum(value) FROM (VALUES (1), (2), (3)) AS values_to_sum(value)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("SQL-defined aggregate").rows(),
        &[vec![Value::Int64(6)]]
    );
}

#[test]
fn should_skip_null_inputs_for_a_strict_sql_defined_aggregate() {
    // Arrange
    let fixture = sql_fixture("issue-910-strict-aggregate-null-inputs", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE AGGREGATE issue_910_strict_sum(INTEGER) \
             (SFUNC = pg_catalog.int4pl, STYPE = INTEGER)",
        )
        .expect("create an aggregate with PostgreSQL's strict int4pl transition");
    let sql = "SELECT \
                 (SELECT issue_910_strict_sum(value) \
                  FROM (VALUES (1), (NULL::INTEGER), (2)) AS mixed(value)), \
                 (SELECT issue_910_strict_sum(value) \
                  FROM (VALUES (NULL::INTEGER), (NULL::INTEGER)) AS all_null(value)), \
                 (SELECT issue_910_strict_sum(value) \
                  FROM (VALUES (1), (2)) AS empty(value) WHERE false)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("strict aggregate ignores NULL inputs").rows(),
        &[vec![Value::Int64(3), Value::Null, Value::Null]]
    );
}

#[test]
fn should_apply_sql_defined_final_function_to_transition_state() {
    // Arrange
    let fixture = sql_fixture("issue-910-aggregate-final-function", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE AGGREGATE issue_910_negative_sum(INTEGER) \
             (SFUNC = pg_catalog.int4pl, STYPE = INTEGER, \
              FINALFUNC = pg_catalog.int4um, INITCOND = '0')",
        )
        .expect("create an aggregate with a unary final function");
    let sql = "SELECT \
                 (SELECT issue_910_negative_sum(value) \
                  FROM (VALUES (1), (2), (3)) AS values_to_sum(value)), \
                 (SELECT issue_910_negative_sum(value) \
                  FROM (VALUES (1)) AS empty(value) WHERE false)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("final function transforms the aggregate transition state")
            .rows(),
        &[vec![Value::Int64(-6), Value::Int64(0)]]
    );
}
