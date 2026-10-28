#[test]
fn should_resolve_text_search_regtype_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-818-text-search-reg-inputs", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture
        .execute(
            "SELECT 'english'::regconfig::oid = 'pg_catalog.english'::regconfig::oid, \
                    'simple'::regdictionary::oid = 'pg_catalog.simple'::regdictionary::oid, \
                    'english'::regconfig::text = 'english', \
                    'simple'::regdictionary::text = 'simple', \
                    pg_typeof('english'::regconfig) = 'regconfig'::regtype, \
                    pg_typeof('simple'::regdictionary) = 'regdictionary'::regtype",
        )
        .expect("resolve text search configuration and dictionary reg* inputs");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
