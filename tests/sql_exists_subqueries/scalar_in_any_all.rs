//! PostgreSQL scalar, row, IN, ANY and ALL subquery behavior tracked by #762.

use cassie::app::CassieError;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config, SqlFixture};

fn subquery_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE subquery_outer (id INT, value INT)",
            "CREATE TABLE subquery_inner (owner_id INT, value INT)",
            "CREATE TABLE subquery_scalar_by_owner (owner_id INT, value INT)",
            "CREATE TABLE subquery_empty (value INT)",
            "CREATE TABLE subquery_values (value INT)",
            "CREATE TABLE subquery_pairs (owner_id INT, left_value INT, right_value INT)",
            "INSERT INTO subquery_outer (id, value) VALUES (1, 100), (2, 200), (3, 300)",
            "INSERT INTO subquery_inner (owner_id, value) VALUES (1, 100), (1, NULL), (2, 200)",
            "INSERT INTO subquery_scalar_by_owner (owner_id, value) VALUES (1, 100), (2, 200)",
            "INSERT INTO subquery_values (value) VALUES (100), (200)",
            "INSERT INTO subquery_pairs (owner_id, left_value, right_value) VALUES (1, 10, 100), (2, 20, 200)",
        ],
    )
}

#[test]
fn should_return_null_when_scalar_subquery_has_no_rows() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-zero-rows");

    // Act
    let result = fixture
        .execute("SELECT (SELECT value FROM subquery_inner WHERE owner_id = 3) AS scalar_value");

    // Assert
    assert_eq!(
        result.expect("zero-row scalar subquery").rows(),
        &[vec![Value::Null]]
    );
}

#[test]
fn should_return_the_value_when_scalar_subquery_has_one_row() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-one-row");

    // Act
    let result = fixture
        .execute("SELECT (SELECT value FROM subquery_inner WHERE owner_id = 2) AS scalar_value");

    // Assert
    assert_eq!(
        result.expect("one-row scalar subquery").rows(),
        &[vec![Value::Int64(200)]]
    );
}

#[test]
fn should_reject_a_scalar_subquery_that_returns_multiple_rows() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-many-rows");

    // Act
    let result = fixture
        .execute("SELECT (SELECT value FROM subquery_inner WHERE owner_id = 1) AS scalar_value");

    // Assert
    assert!(
        matches!(result, Err(CassieError::CardinalityViolation(_))),
        "multiple rows must produce a cardinality violation: {result:?}"
    );
}

#[test]
fn should_reject_a_scalar_subquery_that_returns_multiple_columns() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-many-columns");

    // Act
    let result = fixture
        .execute("SELECT (SELECT left_value, right_value FROM subquery_pairs WHERE owner_id = 1)");

    // Assert
    let error = result.expect_err("scalar subquery with two columns");
    assert!(
        error.to_string().contains("one column"),
        "wrong scalar-subquery arity error: {error}"
    );
}

#[test]
fn should_evaluate_scalar_subqueries_inside_scalar_expressions() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-expression");

    // Act
    let result = fixture
        .execute("SELECT 1 + (SELECT value FROM subquery_inner WHERE owner_id = 2) AS answer");

    // Assert
    assert_eq!(
        result.expect("scalar subquery in arithmetic").rows(),
        &[vec![Value::Int64(201)]]
    );
}

#[test]
fn should_evaluate_scalar_subqueries_in_predicates() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-predicate");

    // Act
    let result = fixture.execute(
        "SELECT id FROM subquery_outer WHERE value < (SELECT value FROM subquery_inner WHERE owner_id = 2) ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("scalar subquery in WHERE").rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_evaluate_correlated_scalar_subqueries_for_each_outer_row() {
    // Arrange
    let fixture = subquery_fixture("issue-762-correlated-scalar");

    // Act
    let result = fixture.execute(
        "SELECT id, (SELECT value FROM subquery_scalar_by_owner WHERE owner_id = subquery_outer.id) AS scalar_value FROM subquery_outer ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("correlated scalar subquery").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(100)],
            vec![Value::Int64(2), Value::Int64(200)],
            vec![Value::Int64(3), Value::Null],
        ]
    );
}

#[test]
fn should_apply_in_subquery_membership_semantics() {
    // Arrange
    let fixture = subquery_fixture("issue-762-in-subquery");

    // Act
    let result = fixture.execute(
        "SELECT 100 IN (SELECT value FROM subquery_values), 150 IN (SELECT value FROM subquery_values), 100 NOT IN (SELECT value FROM subquery_values)",
    );

    // Assert
    assert_eq!(
        result.expect("IN and NOT IN subqueries").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_preserve_null_semantics_for_membership_subqueries() {
    // Arrange
    let fixture = subquery_fixture("issue-762-in-null-semantics");

    // Act
    let result = fixture.execute(
        "SELECT 100 IN (SELECT value FROM subquery_inner WHERE owner_id = 1), 500 IN (SELECT value FROM subquery_inner WHERE owner_id = 1), 500 NOT IN (SELECT value FROM subquery_inner WHERE owner_id = 1)",
    );

    // Assert
    assert_eq!(
        result.expect("IN NULL semantics").rows(),
        &[vec![Value::Bool(true), Value::Null, Value::Null]]
    );
}

#[test]
fn should_apply_empty_membership_subquery_identities() {
    // Arrange
    let fixture = subquery_fixture("issue-762-in-empty");

    // Act
    let result = fixture.execute(
        "SELECT 100 IN (SELECT value FROM subquery_empty), 100 NOT IN (SELECT value FROM subquery_empty)",
    );

    // Assert
    assert_eq!(
        result.expect("empty IN subquery").rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_quantified_comparisons_over_subquery_rows() {
    // Arrange
    let fixture = subquery_fixture("issue-762-any-all");

    // Act
    let result = fixture.execute(
        "SELECT 100 = ANY (SELECT value FROM subquery_values), 100 = ALL (SELECT value FROM subquery_values), 150 < ANY (SELECT value FROM subquery_values), 150 < ALL (SELECT value FROM subquery_values)",
    );

    // Assert
    assert_eq!(
        result.expect("ANY and ALL subqueries").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_apply_empty_identities_for_quantified_subqueries() {
    // Arrange
    let fixture = subquery_fixture("issue-762-any-all-empty");

    // Act
    let result = fixture.execute(
        "SELECT 100 = ANY (SELECT value FROM subquery_empty), 100 = ALL (SELECT value FROM subquery_empty)",
    );

    // Assert
    assert_eq!(
        result.expect("empty ANY and ALL subqueries").rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_null_semantics_for_quantified_subqueries() {
    // Arrange
    let fixture = subquery_fixture("issue-762-any-all-null-semantics");

    // Act
    let result = fixture.execute(
        "SELECT 500 = ANY (SELECT value FROM subquery_inner WHERE owner_id = 1), 100 = ANY (SELECT value FROM subquery_inner WHERE owner_id = 1), 100 = ALL (SELECT value FROM subquery_inner WHERE owner_id = 1)",
    );

    // Assert
    assert_eq!(
        result.expect("ANY and ALL NULL semantics").rows(),
        &[vec![Value::Null, Value::Bool(true), Value::Null]]
    );
}

#[test]
fn should_apply_comparison_operators_to_quantified_subqueries() {
    // Arrange
    let fixture = subquery_fixture("issue-762-any-all-operators");

    // Act
    let result = fixture.execute(
        "SELECT 150 = ANY (SELECT value FROM subquery_values), 150 = ALL (SELECT value FROM subquery_values), 150 <> ANY (SELECT value FROM subquery_values), 150 <> ALL (SELECT value FROM subquery_values), 150 < ANY (SELECT value FROM subquery_values), 150 < ALL (SELECT value FROM subquery_values), 150 <= ANY (SELECT value FROM subquery_values), 150 <= ALL (SELECT value FROM subquery_values), 150 > ANY (SELECT value FROM subquery_values), 150 > ALL (SELECT value FROM subquery_values), 150 >= ANY (SELECT value FROM subquery_values), 150 >= ALL (SELECT value FROM subquery_values)",
    );

    // Assert
    assert_eq!(
        result
            .expect("comparison operators with ANY and ALL")
            .rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_evaluate_correlated_in_subqueries_against_each_outer_row() {
    // Arrange
    let fixture = subquery_fixture("issue-762-correlated-in");

    // Act
    let result = fixture.execute(
        "SELECT id FROM subquery_outer WHERE value IN (SELECT value FROM subquery_inner WHERE owner_id = subquery_outer.id) ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("correlated IN subquery").rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_compare_row_values_with_a_row_subquery() {
    // Arrange
    let fixture = subquery_fixture("issue-762-row-subquery-comparison");

    // Act
    let result = fixture.execute(
        "SELECT ROW(10, 100) = (SELECT left_value, right_value FROM subquery_pairs WHERE owner_id = 1)",
    );

    // Assert
    assert_eq!(
        result.expect("row comparison with subquery").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_match_row_in_subqueries_with_matching_arity() {
    // Arrange
    let fixture = subquery_fixture("issue-762-row-in-subquery");

    // Act
    let result = fixture.execute(
        "SELECT ROW(10, 100) IN (SELECT left_value, right_value FROM subquery_pairs), ROW(10, 200) IN (SELECT left_value, right_value FROM subquery_pairs)",
    );

    // Assert
    assert_eq!(
        result.expect("row IN subquery").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_reject_row_subquery_comparisons_with_different_arities() {
    // Arrange
    let fixture = subquery_fixture("issue-762-row-subquery-arity");

    // Act
    let result = fixture.execute("SELECT ROW(10, 100) = (SELECT value FROM subquery_values)");

    // Assert
    let error = result.expect_err("row and subquery arity mismatch");
    assert!(
        error.to_string().contains("columns") || error.to_string().contains("degree"),
        "wrong row-subquery arity error: {error}"
    );
}

#[test]
fn should_skip_unselected_scalar_subqueries_in_case_expressions() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-lazy-expressions");

    // Act
    let result = fixture.execute(
        "SELECT CASE WHEN TRUE THEN (SELECT value FROM subquery_inner WHERE owner_id = 2) ELSE (SELECT value FROM subquery_inner WHERE owner_id = 1) END",
    );

    // Assert
    assert_eq!(
        result
            .expect("unselected scalar CASE branch is not evaluated")
            .rows(),
        &[vec![Value::Int64(200)]]
    );
}

#[test]
fn should_skip_unselected_scalar_subqueries_in_coalesce() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-lazy-coalesce");

    // Act
    let result = fixture.execute(
        "SELECT COALESCE((SELECT value FROM subquery_inner WHERE owner_id = 2), (SELECT value FROM subquery_inner WHERE owner_id = 1))",
    );

    // Assert
    assert_eq!(
        result
            .expect("unselected COALESCE argument is not evaluated")
            .rows(),
        &[vec![Value::Int64(200)]]
    );
}

#[test]
fn should_evaluate_scalar_subqueries_in_insert_values() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-insert-values");

    // Act
    let inserted = fixture.execute(
        "INSERT INTO subquery_outer (id, value) VALUES (4, (SELECT value FROM subquery_inner WHERE owner_id = 2))",
    );
    let rows = fixture.rows("SELECT value FROM subquery_outer WHERE id = 4");

    // Assert
    assert!(
        inserted.is_ok(),
        "scalar subquery in INSERT values: {inserted:?}"
    );
    assert_eq!(rows, vec![vec![Value::Int64(200)]]);
}

#[test]
fn should_evaluate_scalar_subqueries_in_update_assignments() {
    // Arrange
    let fixture = subquery_fixture("issue-762-scalar-update-assignment");

    // Act
    let updated = fixture.execute(
        "UPDATE subquery_outer SET value = (SELECT value FROM subquery_inner WHERE owner_id = 2) WHERE id = 3",
    );
    let rows = fixture.rows("SELECT value FROM subquery_outer WHERE id = 3");

    // Assert
    assert!(
        updated.is_ok(),
        "scalar subquery in UPDATE assignment: {updated:?}"
    );
    assert_eq!(rows, vec![vec![Value::Int64(200)]]);
}

#[test]
fn should_accept_some_as_an_alias_for_any_subquery_quantification() {
    // Arrange
    let fixture = subquery_fixture("issue-762-some-subquery");

    // Act
    let result = fixture.execute("SELECT 200 = SOME (SELECT value FROM subquery_values)");

    // Assert
    assert_eq!(
        result.expect("SOME subquery quantifier").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_reject_row_in_subqueries_with_different_arities() {
    // Arrange
    let fixture = subquery_fixture("issue-762-row-in-arity");

    // Act
    let result = fixture.execute("SELECT ROW(10, 100) IN (SELECT value FROM subquery_values)");

    // Assert
    let error = result.expect_err("row IN subquery arity mismatch");
    assert!(
        error.to_string().contains("columns") || error.to_string().contains("degree"),
        "wrong row IN arity error: {error}"
    );
}

#[test]
fn should_apply_result_row_limits_to_scalar_subquery_results() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 0;
    let fixture = sql_fixture_with_config(
        "issue-762-scalar-result-limit",
        &[
            "CREATE TABLE limited_values (id INT, value INT)",
            "INSERT INTO limited_values (id, value) VALUES (1, 100)",
        ],
        config,
    );

    // Act
    let result =
        fixture.execute("SELECT (SELECT value FROM limited_values WHERE id = 1) AS scalar_value");

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "scalar subquery must obey the result row cap: {result:?}"
    );
}

#[test]
fn should_refresh_cached_scalar_subquery_results_after_mutation() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::enabled();
    let fixture = sql_fixture_with_config(
        "issue-762-scalar-cache-invalidation",
        &[
            "CREATE TABLE cached_values (id INT, value INT)",
            "INSERT INTO cached_values (id, value) VALUES (1, 100)",
        ],
        config,
    );
    let sql = "SELECT (SELECT value FROM cached_values WHERE id = 1) AS scalar_value";

    // Act
    let before = fixture.execute(sql);
    let updated = fixture.execute("UPDATE cached_values SET value = 101 WHERE id = 1");
    let after = fixture.execute(sql);

    // Assert
    assert!(updated.is_ok(), "update source value: {updated:?}");
    assert_eq!(
        before.expect("initial scalar subquery result").rows(),
        &[vec![Value::Int64(100)]]
    );
    assert_eq!(
        after.expect("scalar subquery after source mutation").rows(),
        &[vec![Value::Int64(101)]]
    );
}
