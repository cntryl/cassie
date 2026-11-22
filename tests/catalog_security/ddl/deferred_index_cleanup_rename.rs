//! A deferred DROP INDEX must still remove the index after its table, column
//! or schema is renamed.
use crate::support_sql::canonical_test_collection;
use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_remove_dropped_index_metadata_after_table_rename() {
    // Arrange
    let fixture = sql_fixture(
        "deferred-index-table-rename",
        &[
            "CREATE TABLE b (id INT, v INT)",
            "INSERT INTO b VALUES (1, 1)",
            "CREATE UNIQUE INDEX ix ON b (v)",
        ],
    );
    let reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("DROP INDEX ix").unwrap();
    fixture.execute("ALTER TABLE b RENAME TO c").unwrap();

    // Act
    drop(reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();

    // Assert
    let collection = canonical_test_collection(&fixture.cassie, "c");
    let stored = fixture.cassie.midge.get_index(&collection, "ix").unwrap();
    assert!(stored.is_none(), "dropped index must not stay in storage");
}

#[test]
fn should_allow_duplicate_rows_after_dropped_index_table_rename() {
    // Arrange
    let fixture = sql_fixture(
        "deferred-index-rename-duplicates",
        &[
            "CREATE TABLE b (id INT, v INT)",
            "INSERT INTO b VALUES (1, 1)",
            "CREATE UNIQUE INDEX ix ON b (v)",
        ],
    );
    let reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("DROP INDEX ix").unwrap();
    fixture.execute("ALTER TABLE b RENAME TO c").unwrap();
    drop(reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();

    // Act
    let inserted = fixture.execute("INSERT INTO c VALUES (2, 1)");

    // Assert
    assert!(
        inserted.is_ok(),
        "dropped unique index must not be enforced"
    );
}

#[test]
fn should_remove_dropped_index_metadata_after_schema_rename() {
    // Arrange
    let fixture = sql_fixture(
        "deferred-index-schema-rename",
        &[
            "CREATE SCHEMA s1",
            "CREATE TABLE s1.b (id INT, v INT)",
            "CREATE UNIQUE INDEX ix ON s1.b (v)",
        ],
    );
    let reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("DROP INDEX s1.ix").unwrap();
    fixture.execute("ALTER SCHEMA s1 RENAME TO s2").unwrap();

    // Act
    drop(reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();

    // Assert
    let collection = canonical_test_collection(&fixture.cassie, "s2.b");
    let leftovers = fixture
        .cassie
        .midge
        .list_indexes()
        .unwrap()
        .into_iter()
        .filter(|index| index.collection == collection)
        .count();
    assert_eq!(leftovers, 0, "dropped index must not stay in storage");
}
