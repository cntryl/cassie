#[test]
fn should_resolve_user_function_overloads_through_to_regprocedure() {
    // Arrange
    let fixture = sql_fixture("issue-818-user-regprocedure-overloads", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_818_routine_lookup")
        .expect("create the overload lookup schema");
    fixture
        .execute(
            "CREATE FUNCTION issue_818_routine_lookup.resolve(INTEGER) \
             RETURNS INTEGER LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
        )
        .expect("create the integer overload");
    fixture
        .execute(
            "CREATE FUNCTION issue_818_routine_lookup.resolve(TEXT) \
             RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT 'x' $$",
        )
        .expect("create the text overload");
    fixture
        .execute("SET search_path TO issue_818_routine_lookup, public")
        .expect("put the custom overloads on search_path");

    // Act
    let result = fixture
        .execute(
            "SELECT to_regprocedure('resolve(integer)')::oid = \
                    to_regprocedure('resolve(int4)')::oid, \
                    to_regprocedure('resolve(integer)')::oid <> \
                    to_regprocedure('resolve(text)')::oid, \
                    to_regprocedure('resolve(bigint)') IS NULL, \
                    to_regprocedure('resolve(integer)')::text = 'resolve(integer)', \
                    to_regprocedure('resolve(text)')::text = 'resolve(text)'",
        )
        .expect("resolve each custom routine signature through search_path");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
