//! Scalar index rename preserves durable identity and constraint ownership.
use crate::support_scalar_index_rebuild::entries;
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_scalar_entries_when_index_is_renamed() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-entries",
        &[
            "CREATE TABLE rename_rows(value INT)",
            "INSERT INTO rename_rows VALUES(7)",
            "CREATE UNIQUE INDEX old_idx ON rename_rows(value)",
        ],
    );
    let before = entries(&fixture.cassie, "rename_rows");
    let oid = fixture.rows("SELECT oid FROM pg_class WHERE relname = 'old_idx'");
    // Act
    fixture
        .execute("ALTER INDEX old_idx RENAME TO new_idx")
        .unwrap();
    // Assert
    assert_eq!(entries(&fixture.cassie, "rename_rows"), before);
    assert_eq!(
        fixture.rows("SELECT oid FROM pg_class WHERE relname = 'new_idx'"),
        oid
    );
    assert!(fixture
        .execute("INSERT INTO rename_rows VALUES(7)")
        .is_err());
    fixture
        .execute("INSERT INTO rename_rows VALUES(11)")
        .unwrap();
    fixture.execute("REINDEX INDEX new_idx").unwrap();
    fixture.execute("DROP INDEX new_idx").unwrap();
    fixture
        .execute("INSERT INTO rename_rows VALUES(7)")
        .unwrap();
}

#[test]
fn should_rename_constraint_ownership_with_backing_index() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-constraint",
        &[
            "CREATE TABLE rename_parent(value INT CONSTRAINT old_key PRIMARY KEY)",
            "CREATE TABLE rename_child(value INT REFERENCES rename_parent(value))",
            "INSERT INTO rename_parent VALUES(7)",
        ],
    );
    // Act
    fixture
        .execute("ALTER INDEX old_key RENAME TO new_key")
        .unwrap();
    // Assert
    assert_eq!(
        fixture.rows("SELECT conname FROM pg_constraint WHERE conname = 'new_key'"),
        vec![vec![Value::String("new_key".into())]]
    );
    assert!(fixture
        .execute("INSERT INTO rename_parent VALUES(7)")
        .is_err());
    fixture
        .execute("INSERT INTO rename_child VALUES(7)")
        .unwrap();
    assert!(fixture
        .execute("DELETE FROM rename_parent WHERE value = 7")
        .is_err());
    assert!(fixture.execute("DROP INDEX new_key").is_err());
}

#[test]
fn should_restore_original_index_name_after_savepoint_rollback() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-rollback",
        &[
            "CREATE TABLE rename_tx(value INT)",
            "CREATE INDEX old_idx ON rename_tx(value)",
            "BEGIN",
            "SAVEPOINT before_rename",
        ],
    );
    // Act
    fixture
        .execute("ALTER INDEX old_idx RENAME TO new_idx")
        .unwrap();
    fixture.execute("ROLLBACK TO before_rename").unwrap();
    fixture.execute("COMMIT").unwrap();
    // Assert
    assert!(fixture
        .cassie
        .catalog
        .get_index("rename_tx", "old_idx")
        .is_some());
    assert!(fixture
        .cassie
        .catalog
        .get_index("rename_tx", "new_idx")
        .is_none());
}

#[test]
fn should_reject_same_schema_index_rename_collision() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-collision",
        &[
            "CREATE TABLE rename_left(value INT)",
            "CREATE TABLE rename_right(value INT)",
            "CREATE INDEX old_idx ON rename_left(value)",
            "CREATE INDEX occupied_idx ON rename_right(value)",
        ],
    );
    // Act
    let result = fixture.execute("ALTER INDEX old_idx RENAME TO occupied_idx");
    // Assert
    assert!(result.is_err());
    assert!(fixture
        .cassie
        .catalog
        .get_index("rename_left", "old_idx")
        .is_some());
}

#[test]
fn should_refresh_materialized_view_using_renamed_index() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-materialized",
        &[
            "CREATE TABLE rename_source(value INT)",
            "INSERT INTO rename_source VALUES(7)",
            "CREATE MATERIALIZED VIEW rename_view AS SELECT value FROM rename_source",
            "CREATE UNIQUE INDEX old_idx ON rename_view(value)",
            "REFRESH MATERIALIZED VIEW CONCURRENTLY rename_view",
        ],
    );
    // Act
    fixture
        .execute("ALTER INDEX old_idx RENAME TO new_idx")
        .unwrap();
    fixture
        .execute("INSERT INTO rename_source VALUES(11)")
        .unwrap();
    fixture
        .execute("REFRESH MATERIALIZED VIEW CONCURRENTLY rename_view")
        .unwrap();
    // Assert
    assert_eq!(
        fixture.rows("SELECT value FROM rename_view ORDER BY value"),
        vec![vec![Value::Int64(7)], vec![Value::Int64(11)]]
    );
    fixture.execute("REINDEX INDEX new_idx").unwrap();
    fixture.execute("DROP INDEX new_idx").unwrap();
}

#[test]
fn should_recover_renamed_constraint_index_after_local_restart() {
    // Arrange
    let directory = crate::support_catalog_directory::CatalogDirectory::new("index-rename-restart");
    let before = {
        let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
            &directory.0,
            cassie::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rename_rows(value INT CONSTRAINT old_key UNIQUE)",
            "INSERT INTO rename_rows VALUES(7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        let before = entries(&cassie, "rename_rows");
        // Act
        cassie
            .execute_sql(&session, "ALTER INDEX old_key RENAME TO new_key", vec![])
            .unwrap();
        before
    };
    let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
        &directory.0,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    // Assert
    assert_eq!(entries(&cassie, "rename_rows"), before);
    assert!(cassie
        .execute_sql(&session, "INSERT INTO rename_rows VALUES(7)", vec![])
        .is_err());
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE rename_rows DROP CONSTRAINT new_key",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(&session, "INSERT INTO rename_rows VALUES(7)", vec![])
        .unwrap();
}

#[test]
fn should_reject_qualified_index_rename_destination() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-qualified-target",
        &[
            "CREATE TABLE rename_rows(value INT)",
            "CREATE INDEX old_idx ON rename_rows(value)",
        ],
    );
    // Act
    let result = fixture.execute("ALTER INDEX old_idx RENAME TO public.new_idx");
    // Assert
    assert!(result.is_err());
    assert!(fixture
        .cassie
        .catalog
        .get_index("rename_rows", "old_idx")
        .is_some());
}

#[test]
fn should_preserve_constraint_ownership_through_repeated_quoted_renames() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-quoted-constraint",
        &[
            "CREATE TABLE rename_rows(value INT CONSTRAINT original_key UNIQUE)",
            "ALTER INDEX original_key RENAME TO \"Renamed.Index\"",
        ],
    );
    // Act
    fixture
        .execute("ALTER INDEX \"Renamed.Index\" RENAME TO final_key")
        .unwrap();
    // Assert
    assert_eq!(
        fixture.rows("SELECT conname FROM pg_constraint WHERE conname = 'final_key'"),
        vec![vec![Value::String("final_key".into())]]
    );
    fixture
        .execute("ALTER TABLE rename_rows DROP CONSTRAINT final_key")
        .unwrap();
}

#[test]
fn should_keep_index_rename_collisions_within_the_owning_schema() {
    // Arrange
    let fixture = sql_fixture(
        "index-rename-schema-collision",
        &[
            "CREATE SCHEMA rename_left",
            "CREATE SCHEMA rename_right",
            "CREATE TABLE rename_left.rows(value INT)",
            "CREATE TABLE rename_right.rows(value INT)",
            "CREATE INDEX old_idx ON rename_left.rows(value)",
            "CREATE INDEX occupied_idx ON rename_right.rows(value)",
        ],
    );
    // Act
    fixture
        .execute("ALTER INDEX rename_left.old_idx RENAME TO occupied_idx")
        .unwrap();
    let collision = fixture.execute("ALTER INDEX rename_left.occupied_idx RENAME TO rows");
    // Assert
    assert!(collision.is_err());
    fixture
        .execute("DROP INDEX rename_left.occupied_idx")
        .unwrap();
    fixture
        .execute("DROP INDEX rename_right.occupied_idx")
        .unwrap();
}
