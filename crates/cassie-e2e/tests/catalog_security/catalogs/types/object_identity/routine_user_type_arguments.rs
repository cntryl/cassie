//! Routines declared with domain, enum and composite argument types: their
//! `regprocedure` identity and the dependency that keeps the type alive
//! (#818, #827, #828). Expected values come from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE DOMAIN arg_dom AS int",
    "CREATE TYPE arg_enum AS ENUM ('a', 'b')",
    "CREATE TYPE arg_pair AS (a int, b text)",
    "CREATE FUNCTION f_dom(a arg_dom) RETURNS INT AS \"1\"",
    "CREATE FUNCTION f_enum(a arg_enum) RETURNS INT AS \"1\"",
    "CREATE FUNCTION f_pair(a arg_pair) RETURNS INT AS \"1\"",
    "CREATE PROCEDURE p_dom(a arg_dom) AS \"SELECT 1\"",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_render_a_function_signature_with_a_domain_argument() {
    // Arrange
    let fixture = fixture("routine_argument_domain_label");

    // Act
    let rows = fixture.rows("SELECT 'f_dom(arg_dom)'::regprocedure::text");

    // Assert
    assert_eq!(rows, vec![vec![text("f_dom(arg_dom)")]]);
}

#[test]
fn should_not_resolve_a_domain_argument_function_by_its_base_type() {
    // Arrange
    let fixture = fixture("routine_argument_domain_base");

    // Act
    let rows = fixture.rows("SELECT to_regprocedure('f_dom(integer)') IS NULL");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_render_a_function_signature_with_an_enum_argument() {
    // Arrange
    let fixture = fixture("routine_argument_enum_label");

    // Act
    let rows = fixture.rows("SELECT 'f_enum(arg_enum)'::regprocedure::text");

    // Assert
    assert_eq!(rows, vec![vec![text("f_enum(arg_enum)")]]);
}

#[test]
fn should_render_a_procedure_signature_with_a_domain_argument() {
    // Arrange
    let fixture = fixture("routine_argument_domain_procedure");

    // Act
    let rows = fixture.rows("SELECT to_regprocedure('p_dom(arg_dom)')::text");

    // Assert
    assert_eq!(rows, vec![vec![text("p_dom(arg_dom)")]]);
}

#[test]
fn should_refuse_to_drop_a_domain_a_function_argument_uses() {
    // Arrange
    let fixture = fixture("routine_argument_domain_dependency");

    // Act
    let error = fixture.error("DROP DOMAIN arg_dom");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_refuse_to_drop_a_composite_type_a_function_argument_uses() {
    // Arrange
    let fixture = fixture("routine_argument_composite_dependency");

    // Act
    let error = fixture.error("DROP TYPE arg_pair");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_drop_a_composite_type_once_the_function_using_it_is_dropped() {
    // Arrange
    let fixture = fixture("routine_argument_composite_released");
    fixture
        .execute("DROP FUNCTION f_pair")
        .expect("drop the dependent function");

    // Act
    let dropped = fixture.execute("DROP TYPE arg_pair");

    // Assert
    assert!(dropped.is_ok());
}
