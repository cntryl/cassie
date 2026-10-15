use super::super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_resolve_user_function_overloads_by_regprocedure_signature() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-regprocedure-user-overloads",
        &[
            "CREATE SCHEMA issue_818_regprocedure_user_overloads",
            "CREATE FUNCTION issue_818_regprocedure_user_overloads.resolve(INTEGER) \
             RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT 'integer overload' $$",
            "CREATE FUNCTION issue_818_regprocedure_user_overloads.resolve(TEXT) \
             RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT 'text overload' $$",
        ],
    );
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("SET search_path TO issue_818_regprocedure_user_overloads, pg_catalog")
        .expect("set the overload schema first");

    // Act
    let selected = fixture
        .execute(
            "SELECT to_regprocedure('resolve(integer)')::oid = \
                    'issue_818_regprocedure_user_overloads.resolve(integer)'::regprocedure::oid, \
                    to_regprocedure('resolve(text)')::oid = \
                    'issue_818_regprocedure_user_overloads.resolve(text)'::regprocedure::oid, \
                    to_regprocedure('resolve(bigint)') IS NULL, \
                    to_regprocedure('resolve(integer)')::oid <> \
                    to_regprocedure('resolve(text)')::oid",
        )
        .expect("resolve both user-defined overload signatures");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
