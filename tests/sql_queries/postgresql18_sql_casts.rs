//! PostgreSQL 18 SQL-defined cast behavior tracked by #911.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

mod support_function_search_path;

#[test]
fn should_apply_implicit_sql_defined_cast_during_function_resolution() {
    // Arrange
    let fixture = sql_fixture("issue-911-sql-defined-cast", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TYPE issue_911_pair AS (n INTEGER)")
        .expect("CREATE TYPE prerequisite should succeed");
    fixture
        .execute("CREATE FUNCTION issue_911_pair_text(issue_911_pair) RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT ($1).n::text $$")
        .expect("cast support function should be created");
    fixture
        .execute("CREATE CAST (issue_911_pair AS TEXT) WITH FUNCTION issue_911_pair_text(issue_911_pair) AS IMPLICIT")
        .expect("CREATE CAST should register its SQL support function");
    fixture
        .execute("CREATE FUNCTION issue_911_echo(TEXT) RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT $1 || '!' $$")
        .expect("echo function should be created");

    // Act
    let result = fixture.execute("SELECT issue_911_echo(ROW(42)::issue_911_pair)");

    // Assert
    assert_eq!(
        result
            .expect("implicit custom cast in function resolution")
            .rows(),
        &[vec![Value::String("42!".to_owned())]]
    );
}

#[test]
fn should_apply_an_explicit_sql_defined_cast_to_a_composite_value() {
    // Arrange
    let fixture = sql_fixture("issue-911-explicit-sql-defined-cast", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TYPE issue_911_explicit_pair AS (n INTEGER)")
        .expect("create a composite source type");
    fixture
        .execute(
            "CREATE FUNCTION issue_911_explicit_pair_text(issue_911_explicit_pair) \
             RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT ($1).n::text $$",
        )
        .expect("create the explicit cast support function");
    fixture
        .execute(
            "CREATE CAST (issue_911_explicit_pair AS TEXT) \
             WITH FUNCTION issue_911_explicit_pair_text(issue_911_explicit_pair)",
        )
        .expect("register the explicit cast");
    let sql = "SELECT CAST(ROW(42)::issue_911_explicit_pair AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("explicit composite-to-text cast").rows(),
        &[vec![Value::String("42".to_owned())]]
    );
}

#[test]
fn should_cast_postgresql_float_special_values_from_text() {
    // Arrange
    let fixture = sql_fixture("issue-796-float-special-value-casts", &[]);
    let cases = [
        ("SELECT CAST('inf' AS FLOAT)::TEXT", "Infinity"),
        ("SELECT CAST('nan' AS FLOAT)::TEXT", "NaN"),
        ("SELECT CAST('-inf' AS FLOAT)::TEXT", "-Infinity"),
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![Value::String((*expected).to_owned())]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL float special-value casts differ:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_truncate_char_casts_to_the_declared_width() {
    // Arrange
    let fixture = sql_fixture("issue-796-char-width-cast", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('abcd' AS CHAR(3))::TEXT");

    // Assert
    assert_eq!(
        result.expect("CHAR(3) truncates its input").rows(),
        &[vec![Value::String("abc".to_owned())]]
    );
}

#[test]
fn should_apply_a_user_defined_inout_assignment_cast_to_jsonb() {
    // Arrange
    let fixture = sql_fixture("issue-911-inout-assignment-cast", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE CAST (INTEGER AS JSONB) WITH INOUT AS ASSIGNMENT")
        .expect("CREATE CAST should register a built-in in/out conversion");
    fixture
        .execute("CREATE TABLE issue_911_assignment_target (payload JSONB)")
        .expect("JSONB target table");

    // Act
    fixture
        .execute("INSERT INTO issue_911_assignment_target VALUES (42)")
        .expect("assignment cast should supply the JSONB column value");
    let selected = fixture
        .execute("SELECT payload FROM issue_911_assignment_target")
        .expect("read cast result");

    // Assert
    assert_eq!(selected.rows(), &[vec![Value::Json(serde_json::json!(42))]]);
}

#[test]
fn should_publish_user_defined_cast_catalog_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-911-cast-catalog-contract", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TYPE issue_911_cast_payload AS (n INTEGER)")
        .expect("create a composite source type");
    fixture
        .execute(
            "CREATE FUNCTION issue_911_cast_payload_text(issue_911_cast_payload) \
             RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT ($1).n::text $$",
        )
        .expect("create the cast support function");
    fixture
        .execute(
            "CREATE CAST (issue_911_cast_payload AS TEXT) \
             WITH FUNCTION issue_911_cast_payload_text(issue_911_cast_payload) \
             AS ASSIGNMENT",
        )
        .expect("register the assignment cast");

    // Act
    let catalog = fixture
        .execute(
            "SELECT source.typname, target.typname, cast.castcontext::text, \
                    cast.castmethod::text, support.proname \
             FROM pg_catalog.pg_cast AS cast \
             JOIN pg_catalog.pg_type AS source ON source.oid = cast.castsource \
             JOIN pg_catalog.pg_type AS target ON target.oid = cast.casttarget \
             JOIN pg_catalog.pg_proc AS support ON support.oid = cast.castfunc \
             WHERE source.typname = 'issue_911_cast_payload' \
               AND target.typname = 'text'",
        )
        .expect("read the created cast's PostgreSQL catalog identity");

    // Assert
    assert_eq!(
        catalog.rows(),
        &[vec![
            Value::String("issue_911_cast_payload".to_owned()),
            Value::String("text".to_owned()),
            Value::String("a".to_owned()),
            Value::String("f".to_owned()),
            Value::String("issue_911_cast_payload_text".to_owned()),
        ]]
    );
}
