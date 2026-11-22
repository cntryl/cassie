//! A user function runs with its caller's privileges: its stored body may read
//! only what the caller could read directly (#1435 follow-up review).

use crate::sequence_state_privileges::{denied, fixture};
use cassie::app::CassieError;
use cassie::types::Value;

const PEEK: &str = "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'";

#[test]
fn should_deny_a_function_whose_body_reads_a_forbidden_table() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT peek() FROM pt");

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_inside_a_window_aggregate_with_a_subquery() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(
        &cassie,
        &alice,
        "SELECT sum(ascii(peek()) + (SELECT 0)) OVER () AS v FROM pt",
    );

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_under_rollup_grouping() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(
        &cassie,
        &alice,
        "SELECT v, max(peek()) FROM pt GROUP BY ROLLUP (v)",
    );

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_in_having() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(
        &cassie,
        &alice,
        "SELECT v FROM pt GROUP BY v HAVING max(peek()) IS NOT NULL",
    );

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_in_order_by() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT v FROM pt ORDER BY peek()");

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_in_an_aggregate_filter() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(
        &cassie,
        &alice,
        "SELECT count(*) FILTER (WHERE peek() IS NOT NULL) FROM pt",
    );

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_inside_case() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(
        &cassie,
        &alice,
        "SELECT CASE WHEN v = 1 THEN peek() END FROM pt",
    );

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_inside_a_lateral_subquery() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[PEEK]);

    // Act
    let refused = denied(
        &cassie,
        &alice,
        "SELECT l.p FROM pt, LATERAL (SELECT peek() AS p) AS l",
    );

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_forbidden_function_body_reached_through_a_nested_call() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[
        PEEK,
        "CREATE FUNCTION outer_peek() RETURNS TEXT AS \"peek()\"",
    ]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT outer_peek() FROM pt");

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_function_body_joining_a_forbidden_table_to_a_readable_one() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[
        "CREATE FUNCTION joined() RETURNS BIGINT AS \"(SELECT count(*) FROM pt JOIN secret ON TRUE)\"",
    ]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT joined()");

    // Assert
    assert!(refused);
}

#[test]
fn should_allow_a_function_body_that_reads_only_a_readable_table() {
    // Arrange
    let (cassie, _root, alice) =
        fixture(&["CREATE FUNCTION top() RETURNS INT AS '(SELECT max(v) FROM pt)'"]);

    // Act
    let result = cassie.execute_sql(
        &alice,
        "SELECT sum(top() + (SELECT 0)) OVER () AS v FROM pt",
        vec![],
    );

    // Assert
    assert!(result.is_ok_and(|result| result.rows() == vec![vec![Value::Int64(1)]]));
}

#[test]
fn should_terminate_authorizing_a_self_referencing_function_body() {
    // Arrange
    let (cassie, _root, alice) =
        fixture(&["CREATE FUNCTION loopy(n INT) RETURNS INT AS \"(CASE WHEN n > 0 THEN loopy(n - 1) ELSE 0 END)\""]);

    // Act
    let result = cassie.execute_sql(&alice, "SELECT loopy(2)", vec![]);

    // Assert
    assert!(!matches!(result, Err(CassieError::InsufficientPrivilege)));
}
