//! `regproc` and `regprocedure` over user-defined routines (#818).
//! Expected values come from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE SCHEMA first_schema",
    "CREATE SCHEMA second_schema",
    "CREATE FUNCTION rate(x INT) RETURNS INT AS \"x * 1000\"",
    "CREATE FUNCTION first_schema.pick(x INT) RETURNS INT AS \"x + 10\"",
    "CREATE FUNCTION second_schema.pick(x TEXT) RETURNS INT AS \"1\"",
    "CREATE FUNCTION first_schema.twin(x INT) RETURNS INT AS \"x + 10\"",
    "CREATE FUNCTION second_schema.twin(x INT) RETURNS INT AS \"x + 20\"",
    "CREATE PROCEDURE refresh_cache(note TEXT) AS \"SELECT 1\"",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn single_text(fixture: &SqlFixture, sql: &str) -> Value {
    fixture.rows(sql).remove(0).remove(0)
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_render_a_resolved_user_function_signature() {
    // Arrange
    let fixture = fixture("user_regprocedure_text");

    // Act
    let rendered = single_text(&fixture, "SELECT 'rate(int4)'::regprocedure::text");

    // Assert
    assert_eq!(rendered, text("rate(integer)"));
}

#[test]
fn should_resolve_a_quoted_user_function_signature() {
    // Arrange
    let fixture = fixture("user_regprocedure_quoted");

    // Act
    let rendered = single_text(
        &fixture,
        "SELECT to_regprocedure('\"rate\"(integer)')::text",
    );

    // Assert
    assert_eq!(rendered, text("rate(integer)"));
}

#[test]
fn should_return_null_for_a_user_function_with_other_argument_types() {
    // Arrange
    let fixture = fixture("user_regprocedure_other_arguments");

    // Act
    let resolved = single_text(&fixture, "SELECT to_regprocedure('rate(text)')");

    // Assert
    assert_eq!(resolved, Value::Null);
}

#[test]
fn should_report_an_undefined_function_for_a_cast_of_a_missing_user_signature() {
    // Arrange
    let fixture = fixture("user_regprocedure_missing");

    // Act
    let error = fixture.error("SELECT 'rate(text)'::regprocedure");

    // Assert
    assert_eq!(error.sql_state(), "42883");
}

#[test]
fn should_resolve_the_same_identity_by_bare_name_and_by_signature() {
    // Arrange
    let fixture = fixture("user_regproc_identity");

    // Act
    let same = single_text(
        &fixture,
        "SELECT 'rate'::regproc::oid = 'rate(integer)'::regprocedure::oid",
    );

    // Assert
    assert_eq!(same, Value::Bool(true));
}

#[test]
fn should_render_a_unique_user_function_name_without_a_schema() {
    // Arrange
    let fixture = fixture("user_regproc_text");

    // Act
    let rendered = single_text(&fixture, "SELECT 'rate'::regproc::text");

    // Assert
    assert_eq!(rendered, text("rate"));
}

#[test]
fn should_keep_a_user_function_identity_stable_across_lookups() {
    // Arrange
    let fixture = fixture("user_regproc_stable_oid");

    // Act
    let same = single_text(
        &fixture,
        "SELECT to_regprocedure('rate(integer)')::oid = 'rate'::regproc::oid",
    );

    // Assert
    assert_eq!(same, Value::Bool(true));
}

#[test]
fn should_resolve_a_user_procedure_signature() {
    // Arrange
    let fixture = fixture("user_regprocedure_procedure");

    // Act
    let rendered = single_text(&fixture, "SELECT 'refresh_cache(text)'::regprocedure::text");

    // Assert
    assert_eq!(rendered, text("refresh_cache(text)"));
}

#[test]
fn should_qualify_a_user_function_outside_the_search_path() {
    // Arrange
    let fixture = fixture("user_regprocedure_qualified_output");

    // Act
    let rendered = single_text(
        &fixture,
        "SELECT 'first_schema.pick(integer)'::regprocedure::text",
    );

    // Assert
    assert_eq!(rendered, text("first_schema.pick(integer)"));
}

#[test]
fn should_report_ambiguity_for_a_bare_name_overloaded_across_schemas() {
    // Arrange
    let fixture = fixture("user_regproc_ambiguous");
    fixture
        .execute("SET search_path TO first_schema, second_schema")
        .expect("put both schemas on the search path");

    // Act
    let error = fixture.error("SELECT 'pick'::regproc");

    // Assert
    assert_eq!(error.sql_state(), "42725");
}

#[test]
fn should_resolve_a_signature_in_the_schema_that_holds_it() {
    // Arrange
    let fixture = fixture("user_regprocedure_overload_by_signature");
    fixture
        .execute("SET search_path TO first_schema, second_schema")
        .expect("put both schemas on the search path");

    // Act
    let rendered = single_text(&fixture, "SELECT 'pick(text)'::regprocedure::text");

    // Assert
    assert_eq!(rendered, text("pick(text)"));
}

#[test]
fn should_prefer_the_earlier_schema_for_identical_signatures() {
    // Arrange
    let fixture = fixture("user_regproc_shadowing");
    fixture
        .execute("SET search_path TO first_schema, second_schema")
        .expect("put both schemas on the search path");

    // Act
    let same = single_text(
        &fixture,
        "SELECT 'twin'::regproc::oid = 'first_schema.twin(integer)'::regprocedure::oid",
    );

    // Assert
    assert_eq!(same, Value::Bool(true));
}

#[test]
fn should_qualify_a_shadowed_user_function_in_regprocedure_output() {
    // Arrange
    let fixture = fixture("user_regprocedure_shadowed_output");
    fixture
        .execute("SET search_path TO first_schema, second_schema")
        .expect("put both schemas on the search path");

    // Act
    let rendered = single_text(
        &fixture,
        "SELECT 'second_schema.twin(integer)'::regprocedure::text",
    );

    // Assert
    assert_eq!(rendered, text("second_schema.twin(integer)"));
}

#[test]
fn should_not_resolve_a_user_function_through_a_schema_off_the_search_path() {
    // Arrange
    let fixture = fixture("user_regprocedure_off_path");

    // Act
    let resolved = single_text(&fixture, "SELECT to_regprocedure('pick(integer)')");

    // Assert
    assert_eq!(resolved, Value::Null);
}
