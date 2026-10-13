//! PostgreSQL 18 SQL-defined cast behavior tracked by #911.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_apply_implicit_sql_defined_cast_during_function_resolution() {
    // Arrange
    let fixture = sql_fixture(
        "issue-911-sql-defined-cast",
        &[
            "CREATE TYPE issue_911_pair AS (n INTEGER)",
            "CREATE FUNCTION issue_911_pair_text(issue_911_pair) RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT ($1).n::text $$",
            "CREATE CAST (issue_911_pair AS TEXT) WITH FUNCTION issue_911_pair_text(issue_911_pair) AS IMPLICIT",
            "CREATE FUNCTION issue_911_echo(TEXT) RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT $1 || '!' $$",
        ],
    );

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
