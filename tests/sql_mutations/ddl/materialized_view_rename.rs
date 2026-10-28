//! Selected SQL materialized-view rename lifecycle.
use crate::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE TABLE rename_source (value INT)",
    "INSERT INTO rename_source VALUES (7)",
    "CREATE MATERIALIZED VIEW rename_before AS SELECT value FROM rename_source",
];

#[test]
fn should_preserve_materialized_identity_when_renamed_view_is_refreshed() {
    // Arrange
    let fixture = sql_fixture("mv-rename-refresh", SETUP);
    fixture
        .execute("CREATE UNIQUE INDEX rename_index ON rename_before(value)")
        .unwrap();
    let before = fixture
        .cassie
        .catalog
        .get_materialized_projection("rename_before")
        .unwrap();
    let _cached = fixture.rows("SELECT value FROM rename_before");
    // Act
    fixture
        .execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after")
        .unwrap();
    let renamed = fixture.rows("SELECT value FROM rename_after");
    fixture
        .execute("INSERT INTO rename_source VALUES (9)")
        .unwrap();
    fixture
        .execute("REFRESH MATERIALIZED VIEW rename_after")
        .unwrap();
    // Assert
    assert_eq!(renamed, vec![vec![Value::Int64(7)]]);
    assert_eq!(
        fixture.rows("SELECT value FROM rename_after ORDER BY value"),
        vec![vec![Value::Int64(7)], vec![Value::Int64(9)]]
    );
    assert!(matches!(
        fixture.execute("SELECT * FROM rename_before"),
        Err(CassieError::CollectionNotFound(_) | CassieError::CatalogObjectNotFound { .. })
    ));
    assert_eq!(
        fixture.rows("SELECT tablename FROM pg_indexes WHERE indexname = 'rename_index'"),
        vec![vec![Value::String("rename_after".into())]]
    );
    let after = fixture
        .cassie
        .catalog
        .get_materialized_projection("rename_after")
        .unwrap();
    assert_eq!(
        before.versions[0].output_collection,
        after.versions[0].output_collection
    );
}

#[test]
fn should_preserve_unpopulated_identity_when_quoted_materialized_view_is_renamed() {
    // Arrange
    let fixture = sql_fixture(
        "mv-rename-unpopulated",
        &["CREATE MATERIALIZED VIEW public.\"BeforeName\" AS SELECT 3 AS value WITH NO DATA"],
    );
    // Act
    fixture
        .execute("ALTER MATERIALIZED VIEW public.\"BeforeName\" RENAME TO \"After Name\"")
        .unwrap();
    let unpopulated = fixture.execute("SELECT * FROM public.\"After Name\"");
    fixture
        .execute("REFRESH MATERIALIZED VIEW public.\"After Name\"")
        .unwrap();
    // Assert
    assert!(
        unpopulated.is_err(),
        "unpopulated view must still reject reads"
    );
    assert_eq!(
        fixture.rows("SELECT * FROM public.\"After Name\""),
        vec![vec![Value::Int64(3)]]
    );
    fixture
        .execute("DROP MATERIALIZED VIEW public.\"After Name\"")
        .unwrap();
    assert!(fixture
        .execute("SELECT * FROM public.\"After Name\"")
        .is_err());
}

#[test]
fn should_publish_materialized_rename_only_at_transaction_commit() {
    // Arrange
    let fixture = sql_fixture("mv-rename-transaction", SETUP);
    let observer = fixture.cassie.create_session("observer", None);
    // Act
    fixture.execute("BEGIN").unwrap();
    fixture.execute("SAVEPOINT before_rename").unwrap();
    fixture
        .execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after")
        .unwrap();
    let private = fixture.rows("SELECT value FROM rename_after");
    let committed_before = fixture
        .cassie
        .execute_sql(&observer, "SELECT value FROM rename_before", vec![])
        .unwrap();
    fixture.execute("ROLLBACK TO before_rename").unwrap();
    let restored = fixture.rows("SELECT value FROM rename_before");
    fixture
        .execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after")
        .unwrap();
    fixture.execute("COMMIT").unwrap();
    // Assert
    assert_eq!(private, committed_before.rows());
    assert_eq!(restored, private);
    assert!(fixture
        .cassie
        .execute_sql(&observer, "SELECT value FROM rename_before", vec![])
        .is_err());
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&observer, "SELECT value FROM rename_after", vec![])
            .unwrap()
            .rows(),
        private
    );
}

#[test]
fn should_preserve_materialized_names_when_rename_validation_fails() {
    // Arrange
    let fixture = sql_fixture("mv-rename-errors", SETUP);
    fixture
        .execute("CREATE VIEW occupied_view AS SELECT 1 AS value")
        .unwrap();
    // Act
    let collision =
        fixture.execute("ALTER MATERIALIZED VIEW rename_before RENAME TO occupied_view");
    let wrong_kind =
        fixture.execute("ALTER MATERIALIZED VIEW rename_source RENAME TO invalid_name");
    let missing = fixture.execute("ALTER MATERIALIZED VIEW missing_view RENAME TO invalid_name");
    // Assert
    assert!(matches!(collision, Err(CassieError::Diagnostic(error)) if error.sql_state == "42P07"));
    assert!(
        matches!(wrong_kind, Err(CassieError::Diagnostic(error)) if error.sql_state == "42809")
    );
    assert!(matches!(missing, Err(CassieError::CollectionNotFound(_))));
    assert_eq!(
        fixture.rows("SELECT value FROM rename_before"),
        vec![vec![Value::Int64(7)]]
    );
    assert_eq!(
        fixture.rows("SELECT value FROM rename_source"),
        vec![vec![Value::Int64(7)]]
    );
}

#[test]
fn should_reject_unsupported_dependent_definitions_before_materialized_rename() {
    // Arrange
    let fixture = sql_fixture("mv-rename-dependent", SETUP);
    fixture
        .execute("CREATE VIEW rename_dependent AS SELECT value FROM rename_before")
        .unwrap();
    // Act
    let rename = fixture.execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after");
    // Assert
    assert!(matches!(rename, Err(CassieError::Diagnostic(error)) if error.sql_state == "2BP01"));
    assert_eq!(
        fixture.rows("SELECT value FROM rename_dependent"),
        vec![vec![Value::Int64(7)]]
    );
    assert_eq!(
        fixture.rows("SELECT value FROM rename_before"),
        vec![vec![Value::Int64(7)]]
    );
}

#[test]
fn should_recover_committed_materialized_rename_after_local_restart() {
    // Arrange
    let directory = crate::support_catalog_directory::CatalogDirectory::new("mv-rename-restart");
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
                .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for sql in SETUP {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
    }
    let cassie =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    // Assert
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT value FROM rename_after", vec![])
            .unwrap()
            .rows(),
        vec![vec![Value::Int64(7)]]
    );
    assert!(cassie
        .execute_sql(&session, "SELECT value FROM rename_before", vec![])
        .is_err());
}

#[test]
fn should_preserve_original_materialized_view_when_metadata_commit_fails() {
    // Arrange
    let fixture = sql_fixture("mv-rename-storage-failure", SETUP);
    cassie::midge::adapter::set_projection_metadata_persistence_failure_point(true);
    // Act
    let failure = fixture.execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after");
    cassie::midge::adapter::set_projection_metadata_persistence_failure_point(false);
    // Assert
    assert!(failure.is_err());
    assert_eq!(
        fixture.rows("SELECT value FROM rename_before"),
        vec![vec![Value::Int64(7)]]
    );
    assert!(fixture
        .cassie
        .catalog
        .get_materialized_projection("rename_after")
        .is_none());
    let persisted = fixture.cassie.midge.list_projection_metadata().unwrap();
    assert!(persisted
        .iter()
        .any(|metadata| metadata.collection.ends_with(".rename_before")));
    assert!(!persisted
        .iter()
        .any(|metadata| metadata.collection.ends_with(".rename_after")));
}

#[test]
fn should_restore_original_materialized_name_when_transaction_is_rolled_back() {
    // Arrange
    let fixture = sql_fixture("mv-rename-rollback", SETUP);
    // Act
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after")
        .unwrap();
    fixture.execute("ROLLBACK").unwrap();
    // Assert
    assert_eq!(
        fixture.rows("SELECT value FROM rename_before"),
        vec![vec![Value::Int64(7)]]
    );
    assert!(fixture.execute("SELECT value FROM rename_after").is_err());
}

#[test]
fn should_preserve_index_ownership_when_renaming_a_versioned_materialized_view() {
    // Arrange
    let fixture = sql_fixture("mv-rename-concurrent-refresh", SETUP);
    fixture
        .execute("CREATE UNIQUE INDEX versioned_rename_index ON rename_before(value)")
        .unwrap();
    fixture
        .execute("REFRESH MATERIALIZED VIEW CONCURRENTLY rename_before")
        .unwrap();
    let before = fixture
        .cassie
        .catalog
        .get_materialized_projection("rename_before")
        .unwrap();
    // Act
    fixture
        .execute("ALTER MATERIALIZED VIEW rename_before RENAME TO rename_after")
        .unwrap();
    let renamed = fixture
        .cassie
        .catalog
        .get_materialized_projection("rename_after")
        .unwrap();
    fixture
        .execute("INSERT INTO rename_source VALUES (11)")
        .unwrap();
    fixture
        .execute("REFRESH MATERIALIZED VIEW CONCURRENTLY rename_after")
        .unwrap();
    let rows = fixture.rows("SELECT value FROM rename_after ORDER BY value");
    let indexes =
        fixture.rows("SELECT tablename FROM pg_indexes WHERE indexname = 'versioned_rename_index'");
    fixture
        .execute("DROP INDEX versioned_rename_index")
        .unwrap();
    // Assert
    assert_eq!(renamed.active_version, before.active_version);
    assert_eq!(renamed.projection_id, before.projection_id);
    assert_eq!(
        renamed
            .versions
            .iter()
            .map(|version| &version.output_collection)
            .collect::<Vec<_>>(),
        before
            .versions
            .iter()
            .map(|version| &version.output_collection)
            .collect::<Vec<_>>()
    );
    assert_eq!(rows, vec![vec![Value::Int64(7)], vec![Value::Int64(11)]]);
    assert_eq!(indexes, vec![vec![Value::String("rename_after".into())]]);
    assert_eq!(
        fixture.rows("SELECT tablename FROM pg_indexes WHERE indexname = 'versioned_rename_index'"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_resolve_only_the_active_materialized_index_after_concurrent_refresh() {
    // Arrange
    let fixture = sql_fixture("mv-active-index-resolution", SETUP);
    fixture
        .execute("CREATE UNIQUE INDEX active_materialized_index ON rename_before(value)")
        .unwrap();
    fixture
        .execute("REFRESH MATERIALIZED VIEW CONCURRENTLY rename_before")
        .unwrap();
    // Act
    let dropped = fixture.execute("DROP INDEX active_materialized_index");
    // Assert
    assert!(
        dropped.is_ok(),
        "retained inactive output indexes must not make SQL index identity ambiguous: {dropped:?}"
    );
    assert_eq!(
        fixture
            .rows("SELECT tablename FROM pg_indexes WHERE indexname = 'active_materialized_index'"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_refresh_original_sources_after_repeated_quoted_materialized_renames() {
    // Arrange
    let fixture = sql_fixture("mv-rename-repeated-quoted", SETUP);
    // Act
    fixture
        .execute("ALTER MATERIALIZED VIEW rename_before RENAME TO \"After Name\"")
        .unwrap();
    fixture
        .execute("ALTER MATERIALIZED VIEW \"After Name\" RENAME TO \"Final.Name\"")
        .unwrap();
    fixture
        .execute("INSERT INTO rename_source VALUES (13)")
        .unwrap();
    fixture
        .execute("REFRESH MATERIALIZED VIEW \"Final.Name\"")
        .unwrap();
    // Assert
    assert_eq!(
        fixture.rows("SELECT value FROM \"Final.Name\" ORDER BY value"),
        vec![vec![Value::Int64(7)], vec![Value::Int64(13)]]
    );
    assert!(fixture.execute("SELECT value FROM \"After Name\"").is_err());
}
