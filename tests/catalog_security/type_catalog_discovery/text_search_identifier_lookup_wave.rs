//! Search-path and schema-privilege behavior for text-search identifier types.

use super::super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::Cassie;
use cassie::types::Value;

fn shared_text_search_identifiers(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE SCHEMA issue_817_ts_lookup_a",
            "CREATE SCHEMA issue_817_ts_lookup_b",
            "CREATE TEXT SEARCH CONFIGURATION issue_817_ts_lookup_a.shared_configuration \
             (COPY = pg_catalog.english)",
            "CREATE TEXT SEARCH CONFIGURATION issue_817_ts_lookup_b.shared_configuration \
             (COPY = pg_catalog.simple)",
            "CREATE TEXT SEARCH DICTIONARY issue_817_ts_lookup_a.shared_dictionary \
             (TEMPLATE = pg_catalog.simple)",
            "CREATE TEXT SEARCH DICTIONARY issue_817_ts_lookup_b.shared_dictionary \
             (TEMPLATE = pg_catalog.simple)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    fixture
}

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

#[test]
fn should_preserve_text_search_object_oids_on_restart_after_rollback() {
    // Arrange
    super::super::support_sql::use_local_storage();
    let path = super::super::support_sql::data_dir("issue-817-text-search-identifier-restart");
    let _directory = super::TypeCatalogDirectory(path.clone());
    let original_oids = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA issue_817_ts_identity", vec![])
            .expect("create schema for text-search identifiers");
        cassie
            .execute_sql(
                &session,
                "CREATE TEXT SEARCH CONFIGURATION issue_817_ts_identity.original_configuration \
                 (COPY = pg_catalog.simple)",
                vec![],
            )
            .expect("create persistent text-search configuration");
        cassie
            .execute_sql(
                &session,
                "CREATE TEXT SEARCH DICTIONARY issue_817_ts_identity.original_dictionary \
                 (TEMPLATE = pg_catalog.simple)",
                vec![],
            )
            .expect("create persistent text-search dictionary");
        let original = cassie
            .execute_sql(
                &session,
                "SELECT 'issue_817_ts_identity.original_configuration'::regconfig::oid, \
                        'issue_817_ts_identity.original_dictionary'::regdictionary::oid",
                vec![],
            )
            .expect("read original text-search object OIDs");
        let original_oids = original.rows()[0].clone();

        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin rollback identity check");
        cassie
            .execute_sql(
                &session,
                "ALTER TEXT SEARCH CONFIGURATION issue_817_ts_identity.original_configuration \
                 RENAME TO rolled_back_configuration",
                vec![],
            )
            .expect("rename configuration inside transaction");
        cassie
            .execute_sql(
                &session,
                "ALTER TEXT SEARCH DICTIONARY issue_817_ts_identity.original_dictionary \
                 RENAME TO rolled_back_dictionary",
                vec![],
            )
            .expect("rename dictionary inside transaction");
        let during_rename = cassie
            .execute_sql(
                &session,
                "SELECT 'issue_817_ts_identity.rolled_back_configuration'::regconfig::oid, \
                        'issue_817_ts_identity.rolled_back_dictionary'::regdictionary::oid",
                vec![],
            )
            .expect("read identifiers after transactional rename");
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("roll back text-search object renames");
        let after_rollback = cassie
            .execute_sql(
                &session,
                "SELECT 'issue_817_ts_identity.original_configuration'::regconfig::oid, \
                        'issue_817_ts_identity.original_dictionary'::regdictionary::oid",
                vec![],
            )
            .expect("read restored identifiers after rollback");
        assert_eq!(during_rename.rows(), vec![original_oids.clone()]);
        assert_eq!(after_rollback.rows(), vec![original_oids.clone()]);

        cassie
            .execute_sql(
                &session,
                "ALTER TEXT SEARCH CONFIGURATION issue_817_ts_identity.original_configuration \
                 RENAME TO persisted_configuration",
                vec![],
            )
            .expect("rename configuration for restart check");
        cassie
            .execute_sql(
                &session,
                "ALTER TEXT SEARCH DICTIONARY issue_817_ts_identity.original_dictionary \
                 RENAME TO persisted_dictionary",
                vec![],
            )
            .expect("rename dictionary for restart check");
        original_oids
    };

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("restart startup");
    let session = cassie.create_session("tester", None);
    let after_restart = cassie
        .execute_sql(
            &session,
            "SELECT 'issue_817_ts_identity.persisted_configuration'::regconfig::oid, \
                    'issue_817_ts_identity.persisted_dictionary'::regdictionary::oid",
            vec![],
        )
        .expect("read text-search object OIDs after restart");

    // Assert
    assert_eq!(after_restart.rows(), vec![original_oids]);
}
