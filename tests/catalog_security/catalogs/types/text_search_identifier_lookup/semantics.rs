#[test]
fn should_resolve_shadowed_text_search_identifiers_from_first_search_path_schema() {
    // Arrange
    let fixture = shared_text_search_identifiers("issue-817-text-search-identifiers-path-order");
    let expected = vec![vec![
        Value::Bool(true),
        Value::Bool(true),
        Value::Bool(true),
        Value::Bool(true),
    ]];

    // Act
    fixture
        .execute("SET search_path TO issue_817_ts_lookup_a, issue_817_ts_lookup_b, pg_catalog")
        .expect("put the first text-search schema first");
    let first_path = fixture
        .execute(
            "SELECT 'shared_configuration'::regconfig::oid = \
                    'issue_817_ts_lookup_a.shared_configuration'::regconfig::oid, \
                    'shared_configuration'::regconfig::oid <> \
                    'issue_817_ts_lookup_b.shared_configuration'::regconfig::oid, \
                    'shared_dictionary'::regdictionary::oid = \
                    'issue_817_ts_lookup_a.shared_dictionary'::regdictionary::oid, \
                    'shared_dictionary'::regdictionary::oid <> \
                    'issue_817_ts_lookup_b.shared_dictionary'::regdictionary::oid",
        )
        .expect("resolve both text-search identifiers from the first schema");
    fixture
        .execute("SET search_path TO issue_817_ts_lookup_b, issue_817_ts_lookup_a, pg_catalog")
        .expect("put the second text-search schema first");
    let second_path = fixture
        .execute(
            "SELECT 'shared_configuration'::regconfig::oid = \
                    'issue_817_ts_lookup_b.shared_configuration'::regconfig::oid, \
                    'shared_configuration'::regconfig::oid <> \
                    'issue_817_ts_lookup_a.shared_configuration'::regconfig::oid, \
                    'shared_dictionary'::regdictionary::oid = \
                    'issue_817_ts_lookup_b.shared_dictionary'::regdictionary::oid, \
                    'shared_dictionary'::regdictionary::oid <> \
                    'issue_817_ts_lookup_a.shared_dictionary'::regdictionary::oid",
        )
        .expect("resolve both text-search identifiers from the second schema");

    // Assert
    assert_eq!(first_path.rows(), expected);
    assert_eq!(second_path.rows(), expected);
}

#[test]
fn should_skip_text_search_schemas_without_usage_during_identifier_lookup() {
    // Arrange
    let fixture = shared_text_search_identifiers("issue-817-text-search-identifiers-schema-usage");
    fixture
        .execute("CREATE ROLE issue_817_ts_lookup_reader")
        .expect("create restricted reader role");
    fixture
        .execute("REVOKE ALL ON SCHEMA issue_817_ts_lookup_a, issue_817_ts_lookup_b FROM PUBLIC")
        .expect("remove public schema access");
    fixture
        .execute(
            "GRANT USAGE ON SCHEMA issue_817_ts_lookup_a, issue_817_ts_lookup_b \
             TO issue_817_ts_lookup_reader",
        )
        .expect("grant both schemas to the reader");
    fixture
        .execute("REVOKE USAGE ON SCHEMA issue_817_ts_lookup_a FROM issue_817_ts_lookup_reader")
        .expect("hide the earlier schema from the reader");
    let reader = fixture
        .cassie
        .create_session("issue_817_ts_lookup_reader", None);
    fixture
        .cassie
        .execute_sql(
            &reader,
            "SET search_path TO issue_817_ts_lookup_a, issue_817_ts_lookup_b, pg_catalog",
            vec![],
        )
        .expect("set the restricted reader's search path");

    // Act
    let selected = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT 'shared_configuration'::regconfig::oid = \
                    'issue_817_ts_lookup_b.shared_configuration'::regconfig::oid, \
                    'shared_dictionary'::regdictionary::oid = \
                    'issue_817_ts_lookup_b.shared_dictionary'::regdictionary::oid, \
                    to_tsvector('shared_configuration'::regconfig, 'The Cats')::text",
            vec![],
        )
        .expect("resolve text-search identifiers through the accessible schema");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::String("'cats':2 'the':1".to_owned()),
        ]]
    );
}
