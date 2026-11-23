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
fn should_preserve_the_create_error_for_a_select_body_without_language() {
    // Arrange
    let fixture = sql_fixture("routine_select_body_without_language", &[]);

    // Act
    let error = fixture.error("CREATE FUNCTION one2() RETURNS INT AS 'SELECT 1'");

    // Assert
    assert!(error
        .to_string()
        .contains("invalid function body for 'one2'"));
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
fn should_reject_malformed_quoted_function_bodies() {
    // Arrange
    let fixture = sql_fixture("routine_invalid_quoted_bodies", &[]);

    // Act
    let unterminated_single =
        fixture.error("CREATE FUNCTION unfinished_single() RETURNS INT AS 'SELECT 1");
    let unterminated_dollar =
        fixture.error("CREATE FUNCTION unfinished_dollar() RETURNS INT AS $$ SELECT 1");
    let invalid_numeric_tag = fixture.error(
        "CREATE FUNCTION invalid_numeric_tag() RETURNS INT \
         AS $1$ SELECT 1 $1$ LANGUAGE sql",
    );
    let invalid_space_tag = fixture.error(
        "CREATE FUNCTION invalid_space_tag() RETURNS INT \
         AS $bad tag$ SELECT 1 $bad tag$ LANGUAGE sql",
    );
    let empty = fixture.error("CREATE FUNCTION empty_body() RETURNS INT AS ''");

    // Assert
    assert_eq!(unterminated_single.sql_state(), "42601");
    assert_eq!(unterminated_dollar.sql_state(), "42601");
    assert_eq!(invalid_numeric_tag.sql_state(), "42601");
    assert_eq!(invalid_space_tag.sql_state(), "42601");
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
fn should_accept_double_precision_function_signatures() {
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
fn should_accept_zoned_timestamp_function_signatures() {
    // Arrange
    let fixture = sql_fixture("routine_timestamptz_argument", &[]);
    fixture
        .execute("SET TIME ZONE 'UTC'")
        .expect("set UTC time zone");

    // Act
    fixture
        .execute(
            "CREATE FUNCTION ts(x timestamp with time zone) \
             RETURNS timestamp with time zone IMMUTABLE AS \"x\"",
        )
        .expect("create a function with multiword timestamp types");
    let signature = single_text(
        &fixture,
        "SELECT to_regprocedure('ts(timestamp with time zone)')::text",
    );
    let result = fixture.rows(
        "SELECT ts('2020-01-01 00:00:00+00') = \
         '2020-01-01 00:00:00+00'::timestamp with time zone",
    );

    // Assert
    assert_eq!(signature, text("ts(timestamp with time zone)"));
    assert_eq!(result, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_character_varying_function_arguments() {
    // Arrange
    let fixture = sql_fixture("routine_varchar_argument", &[]);

    // Act
    fixture
        .execute(
            "CREATE FUNCTION f(x character varying(3)) \
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

#[test]
fn should_evaluate_a_scalar_subquery_in_a_function_in_the_select_list() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_select_list",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT peek()");

    // Assert
    assert_eq!(result, vec![vec![text("123")]]);
}

#[test]
fn should_evaluate_a_scalar_subquery_in_a_function_in_order_by() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_order_by",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT peek() FROM generate_series(1, 1) g ORDER BY 1");

    // Assert
    assert_eq!(result, vec![vec![text("123")]]);
}

#[test]
fn should_resolve_a_routine_hidden_subquery_in_where() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_where",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT n FROM generate_series(1, 2) s(n) WHERE peek() = '123'");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_resolve_a_routine_hidden_subquery_in_values() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_values",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (9)",
            "CREATE FUNCTION selected_value() RETURNS INT AS '(SELECT v FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("VALUES (selected_value())");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(9)]]);
}

#[test]
fn should_resolve_a_routine_hidden_subquery_in_a_set_function_argument() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_set_function_argument",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (2)",
            "CREATE FUNCTION start_at() RETURNS INT AS '(SELECT v FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT n FROM generate_series(start_at(), 3) s(n)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]);
}

#[test]
fn should_evaluate_a_function_body_subquery_in_a_true_order_by_expression() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_order_expression",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (0)",
            "CREATE FUNCTION order_key(x INT) RETURNS INT \
             AS '((SELECT v FROM secret LIMIT 1) + x)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT n FROM generate_series(1, 2) series(n) \
         ORDER BY order_key(n) DESC",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(2)], vec![Value::Int64(1)]]);
}

#[test]
fn should_evaluate_a_scalar_subquery_in_a_function_in_aggregate_filter() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_aggregate_filter",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT count(*) FILTER (WHERE peek() IS NOT NULL) \
         FROM generate_series(1, 2) g",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_preserve_a_routine_scalar_subquery_in_a_window_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_window_aggregate",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT count(peek()) OVER () \
         FROM generate_series(1, 2) g",
    );

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::Int64(2)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_resolve_routine_scalar_subqueries_in_window_partition_and_order() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_window_partition_order",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (7)",
            "CREATE FUNCTION window_key(x INT) RETURNS INT \
             AS '((SELECT v FROM secret LIMIT 1) + x)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT count(*) OVER (PARTITION BY window_key(n - n) ORDER BY window_key(-n)) \
         FROM generate_series(1, 2) s(n)",
    );

    // Assert
    assert_eq!(
        result,
        vec![vec![Value::Int64(2)], vec![Value::Int64(1)]]
    );
}

#[test]
fn should_skip_a_routine_subquery_argument_when_aggregate_filter_is_false() {
    // Arrange
    let fixture = sql_fixture(
        "routine_aggregate_filter_skips_subquery_argument",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (1), (2)",
            "CREATE FUNCTION multivalued() RETURNS INT AS '(SELECT v FROM secret)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT sum(multivalued()) FILTER (WHERE false) \
         FROM generate_series(1, 1) s(n)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Null]]);
}

#[test]
fn should_skip_a_routine_subquery_argument_in_a_having_aggregate_filter() {
    // Arrange
    let fixture = sql_fixture(
        "routine_having_filter_skips_subquery_argument",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (1), (2)",
            "CREATE FUNCTION multivalued() RETURNS INT AS '(SELECT v FROM secret)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT count(*) \
         FROM generate_series(1, 1) s(n) \
         HAVING sum(multivalued()) FILTER (WHERE false) IS NULL",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_skip_a_routine_subquery_argument_in_an_order_by_aggregate_filter() {
    // Arrange
    let fixture = sql_fixture(
        "routine_order_filter_skips_subquery_argument",
        &[
            "CREATE TABLE secret (v INT)",
            "INSERT INTO secret VALUES (1), (2)",
            "CREATE FUNCTION multivalued() RETURNS INT AS '(SELECT v FROM secret)'",
        ],
    );

    // Act
    let result = fixture.rows(
        "SELECT count(*) \
         FROM generate_series(1, 1) s(n) \
         ORDER BY sum(multivalued()) FILTER (WHERE false)",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_evaluate_a_scalar_subquery_in_a_function_in_case() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_case",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT CASE WHEN TRUE THEN peek() END");

    // Assert
    assert_eq!(result, vec![vec![text("123")]]);
}

#[test]
fn should_evaluate_a_scalar_subquery_in_a_function_in_lateral() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_lateral",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT l.p FROM LATERAL (SELECT peek() AS p) AS l");

    // Assert
    assert_eq!(result, vec![vec![text("123")]]);
}

#[test]
fn should_bind_a_function_parameter_inside_a_scalar_subquery_with_table_columns() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_parameter_scope",
        &[
            "CREATE TABLE routine_source (a INT)",
            "INSERT INTO routine_source VALUES (7)",
            "CREATE FUNCTION add_source(x INT) RETURNS INT AS \
             '(SELECT x + a FROM routine_source LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT add_source(1)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(8)]]);
}

#[test]
fn should_prefer_a_subquery_table_column_over_a_routine_parameter() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_local_column_scope",
        &[
            "CREATE TABLE routine_source (x INT, a INT)",
            "INSERT INTO routine_source VALUES (100, 7)",
            "CREATE FUNCTION add_source(x INT) RETURNS INT AS \
             '(SELECT x + a FROM routine_source LIMIT 1)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT add_source(1)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(107)]]);
}

#[test]
fn should_return_null_when_a_function_scalar_subquery_returns_no_rows() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_empty",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret)'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT peek()");

    // Assert
    assert_eq!(result, vec![vec![Value::Null]]);
}

#[test]
fn should_report_cardinality_violation_when_a_function_scalar_subquery_returns_many_rows() {
    // Arrange
    let fixture = sql_fixture(
        "routine_scalar_subquery_many_rows",
        &[
            "CREATE TABLE secret (ssn TEXT)",
            "INSERT INTO secret VALUES ('123'), ('456')",
            "CREATE FUNCTION peek() RETURNS TEXT AS '(SELECT ssn FROM secret)'",
        ],
    );

    // Act
    let error = fixture.error("SELECT peek()");

    // Assert
    assert_eq!(error.sql_state(), "21000");
    assert!(error
        .to_string()
        .contains("more than one row returned by a subquery used as an expression"));
}

#[test]
fn should_reject_an_unknown_name_in_a_function_body_at_creation() {
    // Arrange
    let fixture = sql_fixture("routine_unknown_body_name", &[]);

    // Act
    let error = fixture.error("CREATE FUNCTION g(x int) RETURNS int AS 'x + a'");

    // Assert
    assert_eq!(error.sql_state(), "42703");
    assert!(error.to_string().contains("column \"a\" does not exist"));
}

#[test]
fn should_accept_function_body_references_to_declared_parameters() {
    // Arrange
    let fixture = sql_fixture("routine_declared_body_parameter", &[]);

    // Act
    fixture
        .execute("CREATE FUNCTION h(x int) RETURNS int AS 'x + 1'")
        .expect("create a function whose body references its argument");
    let result = fixture.rows("SELECT h(1)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_resolve_function_body_parameter_names_case_insensitively() {
    // Arrange
    let fixture = sql_fixture("routine_body_parameter_case", &[]);

    // Act
    fixture
        .execute("CREATE FUNCTION h(x int) RETURNS int AS 'X + 1'")
        .expect("resolve a differently cased parameter reference");
    let result = fixture.rows("SELECT h(1)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_resolve_a_quoted_mixed_case_function_parameter() {
    // Arrange
    let fixture = sql_fixture("routine_quoted_parameter_case", &[]);
    fixture
        .execute(r#"CREATE FUNCTION quoted_arg("X" INT) RETURNS INT AS '"X" + 1'"#)
        .expect("create a function with a quoted mixed-case parameter");

    // Act
    let result = fixture.rows("SELECT quoted_arg(4)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(5)]]);
}

#[test]
fn should_allow_distinct_quoted_and_unquoted_function_arguments() {
    // Arrange
    let fixture = sql_fixture("routine_distinct_quoted_arguments", &[]);
    fixture
        .execute(r#"CREATE FUNCTION mixed_args(x INT, "X" INT) RETURNS INT AS 'x + "X"'"#)
        .expect("create a function with distinct unquoted and quoted parameters");

    // Act
    let result = fixture.rows("SELECT mixed_args(2, 40)");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(42)]]);
}

#[test]
fn should_reject_a_quoted_lowercase_function_argument_duplicate() {
    // Arrange
    let fixture = sql_fixture("routine_quoted_argument_duplicate", &[]);

    // Act
    let error = fixture.error(
        r#"CREATE FUNCTION duplicate_arg(x INT, "x" INT) RETURNS INT AS '1'"#,
    );

    // Assert
    assert!(error.to_string().contains("duplicate argument"));
}

#[test]
fn should_resolve_a_function_parameter_before_a_caller_column_with_the_same_name() {
    // Arrange
    let fixture = sql_fixture(
        "routine_body_parameter_shadows_caller_column",
        &[
            "CREATE TABLE t (x int)",
            "INSERT INTO t VALUES (100)",
            "CREATE FUNCTION k(x int) RETURNS int AS 'x + 1'",
        ],
    );

    // Act
    let result = fixture.rows("SELECT k(1) FROM t");

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(2)]]);
}
