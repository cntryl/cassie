//! A create-index statement that fails validation must not leave a publication
//! record that blocks later index creation or startup.
use crate::support_sql_fixture::sql_fixture;
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::midge::adapter::set_index_publication_failure_point;
use std::collections::BTreeMap;

static FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SETUP: [&str; 3] = [
    "CREATE TABLE fu (id INT PRIMARY KEY, v INT)",
    "INSERT INTO fu VALUES (1, 1), (2, 1)",
    "CREATE TABLE fo (id INT PRIMARY KEY, w INT)",
];

#[test]
fn should_create_other_index_after_unique_index_fails_on_duplicates() {
    // Arrange
    let fixture = sql_fixture("failed-unique-index-other", &SETUP);
    let failure = fixture.execute("CREATE UNIQUE INDEX fu_ix ON fu (v)");

    // Act
    let other = fixture.execute("CREATE INDEX fo_ix ON fo (w)");

    // Assert
    assert!(failure.is_err());
    assert!(other.is_ok(), "later CREATE INDEX must not be blocked");
}

#[test]
fn should_create_unique_index_after_duplicates_are_removed() {
    // Arrange
    let fixture = sql_fixture("failed-unique-index-retry", &SETUP);
    assert!(fixture
        .execute("CREATE UNIQUE INDEX fu_ix ON fu (v)")
        .is_err());
    fixture.execute("DELETE FROM fu WHERE id = 2").unwrap();

    // Act
    let retry = fixture.execute("CREATE UNIQUE INDEX fu_ix ON fu (v)");

    // Assert
    assert!(retry.is_ok(), "retry after cleanup must succeed");
    assert!(fixture.execute("INSERT INTO fu VALUES (3, 1)").is_err());
}

fn unique_index(collection: &str) -> IndexMeta {
    IndexMeta {
        definition_semantics_version: 0,
        collection: collection.to_string(),
        name: "fu_ix".to_string(),
        field: "v".to_string(),
        fields: vec!["v".to_string()],
        expressions: vec![],
        include_fields: vec![],
        predicate: None,
        kind: IndexKind::Scalar,
        unique: true,
        options: BTreeMap::new(),
    }
}

/// Leaves a prepared publication for a unique index over duplicate rows, as a
/// crash between journal commit and rebuild would, then reopens storage.
fn restart_with_stale_publication(label: &str, drop_table: bool) -> Cassie {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in SETUP {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    let collection = crate::support_sql::canonical_test_collection(&cassie, "fu");
    set_index_publication_failure_point(true);
    assert!(cassie.midge.put_index(&unique_index(&collection)).is_err());
    if drop_table {
        cassie
            .execute_sql(&session, "DROP TABLE fu", vec![])
            .unwrap();
    }
    drop(session);
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("startup must survive");
    restarted
}

#[test]
fn should_start_after_prepared_unique_index_fails_validation() {
    // Arrange
    let _guard = FAILPOINT_GUARD.lock().unwrap();

    // Act
    let restarted = restart_with_stale_publication("stale-unique-publication", false);

    // Assert
    let session = restarted.create_session("tester", None);
    let created = restarted.execute_sql(&session, "CREATE INDEX fo_ix ON fo (w)", vec![]);
    assert!(
        created.is_ok(),
        "stale record must not block index creation"
    );
}

#[test]
fn should_start_after_prepared_index_table_was_dropped() {
    // Arrange
    let _guard = FAILPOINT_GUARD.lock().unwrap();

    // Act
    let restarted = restart_with_stale_publication("stale-dropped-publication", true);

    // Assert
    let session = restarted.create_session("tester", None);
    let created = restarted.execute_sql(&session, "CREATE INDEX fo_ix ON fo (w)", vec![]);
    assert!(
        created.is_ok(),
        "stale record must not block index creation"
    );
}
