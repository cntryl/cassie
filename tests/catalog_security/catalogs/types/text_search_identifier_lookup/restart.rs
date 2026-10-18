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
fn should_preserve_text_search_object_oids_on_restart_after_rollback() {
    // Arrange
    super::super::support_sql::use_local_storage();
    let path = super::super::support_sql::data_dir("issue-817-text-search-identifier-restart");
    let _directory = super::TypeCatalogDirectory(path.clone());
    let original_oids = create_text_search_objects_and_rename(std::path::Path::new(&path));

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

fn create_text_search_objects_and_rename(path: &std::path::Path) -> Vec<Value> {
    let cassie = Cassie::new_with_data_dir(path).expect("create Cassie instance");
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
}

include!("semantics.rs");
