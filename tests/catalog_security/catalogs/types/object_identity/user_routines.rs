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
fn should_resolve_a_bare_name_to_the_identity_of_its_signature() {
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

#[test]
fn should_evaluate_a_single_quoted_select_body_with_a_trailing_language_clause() {
    // Arrange
    let fixture = sql_fixture("routine_language_sql_clause", &[]);

    // Act
    fixture
        .execute("CREATE FUNCTION one() RETURNS INT AS 'SELECT 1' LANGUAGE sql")
        .expect("create a SQL function with its language clause");
    let result = fixture.rows("SELECT one()");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_unescape_doubled_quotes_in_a_single_quoted_function_body() {
    // Arrange
    let fixture = sql_fixture("routine_doubled_body_quotes", &["CREATE SEQUENCE ps"]);

    // Act
    fixture
        .execute(
            "CREATE FUNCTION bump() RETURNS BIGINT \
             AS 'SELECT nextval(''ps'')' LANGUAGE sql",
        )
        .expect("create a function with doubled quotes in its body");
    let result = fixture.rows("SELECT bump()");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_reject_unterminated_and_empty_quoted_function_bodies() {
    // Arrange
    let fixture = sql_fixture("routine_invalid_quoted_bodies", &[]);

    // Act
    let unterminated_single =
        fixture.error("CREATE FUNCTION unfinished_single() RETURNS INT AS 'SELECT 1");
    let unterminated_dollar =
        fixture.error("CREATE FUNCTION unfinished_dollar() RETURNS INT AS $$ SELECT 1");
    let empty = fixture.error("CREATE FUNCTION empty_body() RETURNS INT AS ''");

    // Assert
    assert_eq!(unterminated_single.sql_state(), "42601");
    assert_eq!(unterminated_dollar.sql_state(), "42601");
    assert!(empty.to_string().contains("empty function/procedure body"));
}

#[test]
fn should_evaluate_dollar_quoted_function_bodies_as_sql() {
    // Arrange
    let fixture = sql_fixture("routine_dollar_quoted_body", &["CREATE SEQUENCE ps"]);

    // Act
    fixture
        .execute("CREATE FUNCTION bump() RETURNS BIGINT AS $$ nextval('ps') $$")
        .expect("create a dollar-quoted SQL expression body");
    let first = fixture.rows("SELECT bump()");
    fixture
        .execute(
            "CREATE FUNCTION bump2() RETURNS BIGINT \
             AS $$ SELECT nextval('ps') $$ LANGUAGE sql",
        )
        .expect("create a dollar-quoted SELECT body with a language clause");
    let second = fixture.rows("SELECT bump2()");

    // Assert
    assert_eq!(first, vec![vec![Value::Int64(1)]]);
    assert_eq!(second, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_preserve_quotes_inside_tagged_dollar_quoted_function_bodies() {
    // Arrange
    let fixture = sql_fixture("routine_tagged_dollar_body", &[]);

    // Act
    fixture
        .execute(
            "CREATE FUNCTION quote_value() RETURNS TEXT \
             AS $body$ SELECT 'x' $body$ LANGUAGE sql",
        )
        .expect("create a tagged dollar-quoted function");
    let result = fixture.rows("SELECT quote_value()");

    // Assert
    assert_eq!(result, vec![vec![text("x")]]);
}

#[test]
fn should_accept_double_precision_function_arguments_and_returns() {
    // Arrange
    let fixture = sql_fixture("routine_double_precision_types", &[]);

    // Act
    fixture
        .execute(
            "CREATE FUNCTION dp(x double precision) \
             RETURNS double precision AS \"x * 2\"",
        )
        .expect("create a function with double precision argument and result");
    let result = fixture.rows("SELECT dp(1.5)");

    // Assert
    assert_eq!(result, vec![vec![Value::Float64(3.0)]]);
}

#[test]
fn should_accept_a_double_precision_function_result_type() {
    // Arrange
    let fixture = sql_fixture("routine_double_precision_result", &[]);

    // Act
    fixture
        .execute("CREATE FUNCTION rt() RETURNS double precision AS \"1.5\"")
        .expect("create a function with a double precision result");
    let result = fixture.rows("SELECT rt()");

    // Assert
    assert_eq!(result, vec![vec![Value::Float64(1.5)]]);
}

#[test]
fn should_accept_timestamp_with_time_zone_function_arguments_and_results() {
    // Arrange
    let fixture = sql_fixture("routine_timestamptz_argument", &[]);
    fixture.execute("SET TIME ZONE 'UTC'").expect("set UTC time zone");

    // Act
    fixture
        .execute(
            "CREATE FUNCTION ts(x timestamp with time zone) \
             RETURNS timestamp with time zone IMMUTABLE AS \"x\"",
        )
        .expect("create a function with multiword timestamp types");
    let result = fixture.rows(
        "SELECT ts('2020-01-01 00:00:00+00') = \
         '2020-01-01 00:00:00+00'::timestamp with time zone",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_character_varying_function_arguments() {
    // Arrange
    let fixture = sql_fixture("routine_varchar_argument", &[]);

    // Act
    fixture
        .execute(
            "CREATE FUNCTION f(x character varying(10)) \
             RETURNS character varying AS \"x\"",
        )
        .expect("create a function with a length-qualified character type");
    let result = fixture.rows("SELECT f('abcdef')");

    // Assert
    assert_eq!(result, vec![vec![text("abcdef")]]);
}

#[test]
fn should_reject_an_unknown_function_argument_type() {
    // Arrange
    let fixture = sql_fixture("routine_unknown_argument_type", &[]);

    // Act
    let error = fixture.error(
        "CREATE FUNCTION unknown_type(x missing_routine_type) \
         RETURNS INT AS \"1\"",
    );

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_keep_single_word_function_volatility_clauses() {
    // Arrange
    let fixture = sql_fixture("routine_volatility_after_single_word_types", &[]);

    // Act
    fixture
        .execute("CREATE FUNCTION stable_one() RETURNS int IMMUTABLE AS \"1\"")
        .expect("parse IMMUTABLE after one-word type");
    fixture
        .execute("CREATE FUNCTION volatile_one() RETURNS int VOLATILE AS \"2\"")
        .expect("parse VOLATILE after one-word type");
    let result = fixture.rows("SELECT stable_one(), volatile_one()");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(1), Value::Int64(2)]]);
}

#[test]
fn should_accept_array_types_after_multiword_routine_types() {
    // Arrange
    let fixture = sql_fixture("routine_multiword_array_type", &[]);

    // Act
    fixture
        .execute(
            "CREATE FUNCTION identity_array(x double precision[]) \
             RETURNS double precision[] AS \"x\"",
        )
        .expect("create a function with a multiword array type");

    // Assert
    assert!(fixture.execute("SELECT identity_array(ARRAY[1.5])").is_ok());
}
