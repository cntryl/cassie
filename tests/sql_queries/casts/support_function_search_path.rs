use super::*;

#[test]
fn should_bind_sql_cast_support_function_at_creation_path() {
    // Arrange
    let fixture = sql_fixture("issue-911-cast-support-search-path", &[]);
    fixture.cassie.startup().expect("startup");
    for statement in [
        "CREATE SCHEMA issue_911_cast_path_left",
        "CREATE SCHEMA issue_911_cast_path_right",
        "CREATE TYPE public.issue_911_cast_path_payload AS (value INTEGER)",
        "CREATE FUNCTION issue_911_cast_path_left.cast_path_value(\
           public.issue_911_cast_path_payload) RETURNS INTEGER \
           LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
        "CREATE FUNCTION issue_911_cast_path_right.cast_path_value(\
           public.issue_911_cast_path_payload) RETURNS INTEGER \
           LANGUAGE SQL IMMUTABLE AS $$ SELECT 2 $$",
        "SET search_path TO issue_911_cast_path_left, \
         issue_911_cast_path_right, public",
        "CREATE CAST (public.issue_911_cast_path_payload AS INTEGER) \
         WITH FUNCTION cast_path_value(public.issue_911_cast_path_payload) \
         AS ASSIGNMENT",
    ] {
        fixture
            .execute(statement)
            .unwrap_or_else(|error| panic!("setup statement failed: {statement}: {error}"));
    }

    // Act
    let selected_routine = fixture
        .execute(
            "SELECT routine_namespace.nspname \
             FROM pg_catalog.pg_cast AS cast_row \
             JOIN pg_catalog.pg_type AS source ON source.oid = cast_row.castsource \
             JOIN pg_catalog.pg_type AS target ON target.oid = cast_row.casttarget \
             JOIN pg_catalog.pg_proc AS support ON support.oid = cast_row.castfunc \
             JOIN pg_catalog.pg_namespace AS routine_namespace \
               ON routine_namespace.oid = support.pronamespace \
             WHERE source.typname = 'issue_911_cast_path_payload' \
               AND target.typname = 'int4'",
        )
        .expect("read the SQL cast's bound support-function schema");
    fixture
        .execute(
            "SET search_path TO issue_911_cast_path_right, \
                  issue_911_cast_path_left, public",
        )
        .expect("reverse the function search path after cast creation");
    let cast_value = fixture
        .execute("SELECT CAST(ROW(42)::public.issue_911_cast_path_payload AS INTEGER)")
        .expect("apply the cast after changing search_path");

    // Assert
    assert_eq!(
        selected_routine.rows(),
        &[vec![Value::String("issue_911_cast_path_left".to_owned())]]
    );
    assert_eq!(cast_value.rows(), &[vec![Value::Int64(1)]]);
}
