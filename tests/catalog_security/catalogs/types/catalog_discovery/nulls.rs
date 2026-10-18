#[test]
fn should_resolve_regtype_aliases_with_nullable_lookup() {
    // Arrange
    let fixture = sql_fixture("regtype-alias-and-missing-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT 'pg_catalog.int4'::regtype, 'integer'::regtype = 'int4'::regtype, to_regtype('type_that_does_not_exist') IS NULL",
        )
        .expect("regtype alias and nullable lookup");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("integer".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_return_null_for_missing_regtype_aliases() {
    // Arrange
    let fixture = sql_fixture("regtype-alias-and-missing-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT 'pg_catalog.int4'::regtype::oid, 'integer'::regtype = 'int4'::regtype, to_regtype('type_that_does_not_exist') IS NULL",
        )
        .expect("regtype alias and nullable lookup");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(23), Value::Bool(true), Value::Bool(true),]]
    );
}

#[test]
fn should_return_null_for_missing_regnamespace_lookup() {
    // Arrange
    let fixture = sql_fixture("issue-818-regnamespace-missing-lookup", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regnamespace('missing_schema') IS NULL")
        .expect("missing regnamespace lookup");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_null_for_a_missing_regproc_name() {
    // Arrange
    let fixture = sql_fixture("issue-818-regproc-regrole-missing-names", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regproc('pg_catalog.no_such_function') IS NULL")
        .expect("return NULL for a missing procedure name");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_null_for_a_missing_regrole_name() {
    // Arrange
    let fixture = sql_fixture("issue-818-regrole-missing-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regrole('issue_818_missing_role') IS NULL")
        .expect("return NULL for a missing role name");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
