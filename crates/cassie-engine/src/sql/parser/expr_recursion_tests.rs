use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use cassie_core::types::Value;

fn in_memory_session(name: &str) -> (Cassie, crate::app::CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session(name, None);
    (cassie, session)
}

#[test]
fn should_keep_session_usable_after_logical_chain_limit_rejection() {
    // Arrange
    let (cassie, session) = in_memory_session("logical_chain_probe");
    let over_limit = format!("SELECT {}", vec!["true"; 129].join(" OR "));

    // Act
    let error = cassie
        .execute_sql(&session, &over_limit, vec![])
        .expect_err("over-limit OR chain must fail as a query error");
    let follow_up = cassie.execute_sql(&session, "SELECT 1", vec![]);

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert_eq!(
        follow_up
            .expect("same session survives parser rejection")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
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
