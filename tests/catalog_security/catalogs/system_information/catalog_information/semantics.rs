#[test]
fn should_list_the_builtin_simple_text_search_configuration() {
    // Arrange
    let fixture = sql_fixture("issue-817-text-search-config-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT cfgname FROM pg_catalog.pg_ts_config WHERE cfgname = 'simple'")
        .expect("query the built-in text search configuration catalog");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("simple".to_owned())]]
    );
}
