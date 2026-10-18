#[test]
fn should_scope_object_identity_lookups_to_the_active_database() {
    // Arrange
    let fixture = sql_fixture("issue-818-object-identity-database-scope", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_818_object_scope")
        .expect("create the current database schema");
    fixture
        .execute("CREATE TYPE issue_818_object_scope.scope_state AS ENUM ('current')")
        .expect("create the current database type");
    fixture
        .execute(
            "CREATE FUNCTION issue_818_object_scope.resolve(INTEGER) \
             RETURNS INTEGER LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
        )
        .expect("create the current database routine");
    fixture
        .execute("CREATE DATABASE issue_818_object_scope_other")
        .expect("create the second database");
    let other_database = fixture
        .cassie
        .create_session("tester", Some("issue_818_object_scope_other".to_owned()));

    // Act
    let current = fixture
        .execute(
            "SELECT to_regnamespace('issue_818_object_scope') IS NOT NULL, \
                    to_regtype('issue_818_object_scope.scope_state') IS NOT NULL, \
                    to_regprocedure('issue_818_object_scope.resolve(integer)') IS NOT NULL",
        )
        .expect("resolve all three identities in their owning database");
    let other = fixture
        .cassie
        .execute_sql(
            &other_database,
            "SELECT to_regnamespace('issue_818_object_scope') IS NULL, \
                    to_regtype('issue_818_object_scope.scope_state') IS NULL, \
                    to_regprocedure('issue_818_object_scope.resolve(integer)') IS NULL",
            vec![],
        )
        .expect("do not resolve identities from another database");

    // Assert
    assert_eq!(
        current.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
    assert_eq!(
        other.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}
