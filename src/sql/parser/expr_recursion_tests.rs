use super::parse_statement;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::sql::SqlErrorKind;
use crate::types::Value;

#[test]
fn should_reject_not_chain_before_parser_recursion_exceeds_bound() {
    // Arrange
    let sql = format!("SELECT {}true", "NOT ".repeat(50_000));

    // Act
    let error = parse_statement(&sql).expect_err("deep NOT chain must be rejected");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("NOT nesting exceeds 128"));
}
#[test]
fn should_accept_exact_not_depth_boundary() {
    // Arrange
    let sql = format!("SELECT {}true", "NOT ".repeat(128));

    // Act
    let result = parse_statement(&sql);

    // Assert
    assert!(result.is_ok(), "128 NOT tokens are within the contract");
}

#[test]
fn should_reject_not_chain_at_one_beyond_depth_boundary() {
    // Arrange
    let sql = format!("SELECT {}true", "NOT ".repeat(129));

    // Act
    let error = parse_statement(&sql).expect_err("129 NOT tokens exceed the contract");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("NOT nesting exceeds 128"));
}

#[test]
fn should_reject_case_chain_before_parser_recursion_exceeds_bound() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "CASE WHEN true THEN ".repeat(3_000),
        " END".repeat(3_000)
    );

    // Act
    let error = parse_statement(&sql).expect_err("deep CASE chain must be rejected");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("CASE nesting exceeds 128"));
}

#[test]
fn should_accept_exact_case_depth_boundary() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "CASE WHEN true THEN ".repeat(128),
        " END".repeat(128)
    );

    // Act
    let result = parse_statement(&sql);

    // Assert
    assert!(result.is_ok(), "128 CASE levels are within the contract");
}

#[test]
fn should_reject_case_chain_at_one_beyond_depth_boundary() {
    // Arrange
    let sql = format!(
        "SELECT {}1{}",
        "CASE WHEN true THEN ".repeat(129),
        " END".repeat(129)
    );

    // Act
    let error = parse_statement(&sql).expect_err("129 CASE levels exceed the contract");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    assert!(error.to_string().contains("CASE nesting exceeds 128"));
}

#[test]
fn should_probe_array_nesting_and_retain_the_same_session() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("array_probe", None);
    let sql = format!(
        "SELECT {}1{}",
        "ARRAY[".repeat(256),
        "]".repeat(256)
    );

    // Act
    let result = cassie.execute_sql(&session, &sql, vec![]);
    let follow_up = cassie.execute_sql(&session, "SELECT 1", vec![]);

    // Assert
    match result {
        Ok(_) | Err(_) => {}
    }
    let follow_up = follow_up.expect("same session must survive the ARRAY probe");
    assert_eq!(follow_up.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_preserve_shallow_expression_and_acyclic_routine_behavior() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("shallow_recursion", None);

    // Act
    let not_result = cassie
        .execute_sql(&session, "SELECT NOT NOT NOT true", vec![])
        .expect("shallow NOT chain");
    let case_result = cassie
        .execute_sql(
            &session,
            "SELECT CASE WHEN true THEN CASE WHEN false THEN 1 ELSE 2 END END",
            vec![],
        )
        .expect("shallow CASE chain");
    cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION rm_c(a int) RETURNS int AS 'a'",
            vec![],
        )
        .expect("leaf function");
    cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION rm_b(a int) RETURNS int AS 'rm_c(a)'",
            vec![],
        )
        .expect("middle function");
    cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION rm_a(a int) RETURNS int AS 'rm_b(a)'",
            vec![],
        )
        .expect("root function");
    let function_result = cassie
        .execute_sql(&session, "SELECT rm_a(3)", vec![])
        .expect("acyclic routine chain");

    // Assert
    assert_eq!(not_result.rows(), &[vec![Value::Bool(false)]]);
    assert_eq!(case_result.rows(), &[vec![Value::Int64(2)]]);
    assert_eq!(function_result.rows(), &[vec![Value::Int64(3)]]);
}

#[test]
fn should_reject_direct_self_call_and_preserve_the_session() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("direct_self_call", None);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "CREATE FUNCTION f(a int) RETURNS int AS 'f(a)'",
            vec![],
        )
        .expect_err("direct self-call must remain rejected");
    let follow_up = cassie
        .execute_sql(&session, "SELECT 1", vec![])
        .expect("session survives direct self-call rejection");

    // Assert
    assert!(error
        .to_string()
        .contains("function 'f' cannot call itself"));
    assert_eq!(follow_up.rows(), &[vec![Value::Int64(1)]]);
}
