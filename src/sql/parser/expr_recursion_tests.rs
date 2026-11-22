use super::parse_statement;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::sql::SqlErrorKind;
use crate::types::Value;

fn in_memory_session(name: &str) -> (Cassie, crate::app::CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session(name, None);
    (cassie, session)
}

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
fn should_parse_array_at_exact_nesting_boundary() {
    // Arrange
    let sql = format!("SELECT {}1{}", "ARRAY[".repeat(128), "]".repeat(128));

    // Act
    let result = parse_statement(&sql);

    // Assert
    assert!(result.is_ok(), "128 ARRAY levels are within the contract");
}

#[test]
fn should_keep_session_usable_after_array_nesting_probe() {
    // Arrange
    let (cassie, session) = in_memory_session("array_probe");
    let sql = format!("SELECT {}1{}", "ARRAY[".repeat(256), "]".repeat(256));

    // Act
    let error = cassie
        .execute_sql(&session, &sql, vec![])
        .expect_err("deep ARRAY nesting must be rejected before stack recursion");
    let follow_up = cassie.execute_sql(&session, "SELECT 1", vec![]);

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert!(error.to_string().contains("SQL nesting exceeds 128"));
    let follow_up = follow_up.expect("same session must survive the ARRAY probe");
    assert_eq!(follow_up.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_keep_session_usable_after_deep_array_function_body_rejection() {
    // Arrange
    let (cassie, session) = in_memory_session("array_function_probe");
    let body = format!("ARRAY[{}1{}]", "[".repeat(255), "]".repeat(255));
    let sql = format!("CREATE FUNCTION array_depth() RETURNS int AS '{body}'");

    // Act
    let error = cassie
        .execute_sql(&session, &sql, vec![])
        .expect_err("deep ARRAY function body must be rejected before recursion");
    let follow_up = cassie.execute_sql(&session, "SELECT 1", vec![]);

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert!(error.to_string().contains("SQL nesting exceeds 128"));
    assert_eq!(
        follow_up
            .expect("same session must survive function-body rejection")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_evaluate_shallow_not_expression() {
    // Arrange
    let (cassie, session) = in_memory_session("shallow_not");

    // Act
    let not_result = cassie
        .execute_sql(&session, "SELECT NOT NOT NOT true", vec![])
        .expect("shallow NOT chain");

    // Assert
    assert_eq!(not_result.rows(), &[vec![Value::Bool(false)]]);
}

#[test]
fn should_evaluate_shallow_case_expression() {
    // Arrange
    let (cassie, session) = in_memory_session("shallow_case");

    // Act
    let case_result = cassie
        .execute_sql(
            &session,
            "SELECT CASE WHEN true THEN CASE WHEN false THEN 1 ELSE 2 END END",
            vec![],
        )
        .expect("shallow CASE chain");

    // Assert
    assert_eq!(case_result.rows(), &[vec![Value::Int64(2)]]);
}

#[test]
fn should_evaluate_acyclic_routine_chain() {
    // Arrange
    let (cassie, session) = in_memory_session("acyclic_routine");

    // Act
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
    assert_eq!(function_result.rows(), &[vec![Value::Int64(3)]]);
}

#[test]
fn should_keep_session_usable_after_direct_self_call_rejection() {
    // Arrange
    let (cassie, session) = in_memory_session("direct_self_call");

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
