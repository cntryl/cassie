use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use std::collections::HashSet;

use super::support_sql as support;

#[test]
fn should_not_resolve_a_renamed_column_alias_after_adding_its_old_name() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("renamed_column_alias_reuse");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE renamed_column_alias_reuse (a INT, b INT)",
        "INSERT INTO renamed_column_alias_reuse (a, b) VALUES (1, 42)",
        "ALTER TABLE renamed_column_alias_reuse RENAME COLUMN b TO c",
        "ALTER TABLE renamed_column_alias_reuse ADD COLUMN b TEXT",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    let explicit = cassie
        .execute_sql(
            &session,
            "SELECT a, b, c FROM renamed_column_alias_reuse",
            vec![],
        )
        .expect("select explicit columns");
    let wildcard = cassie
        .execute_sql(&session, "SELECT * FROM renamed_column_alias_reuse", vec![])
        .expect("select wildcard columns");
    let stale_alias = cassie
        .execute_sql(
            &session,
            "SELECT a FROM renamed_column_alias_reuse WHERE b = '42'",
            vec![],
        )
        .expect("filter by newly added column");

    // Assert
    assert_eq!(
        explicit.rows(),
        vec![vec![Value::Int64(1), Value::Null, Value::Int64(42)]]
    );
    assert_eq!(wildcard.rows().len(), 1);
    let column_position = |name: &str| {
        wildcard
            .columns()
            .iter()
            .position(|column| column.name == name)
            .expect("wildcard column")
    };
    assert_eq!(wildcard.rows()[0][column_position("a")], Value::Int64(1));
    assert_eq!(wildcard.rows()[0][column_position("b")], Value::Null);
    assert_eq!(wildcard.rows()[0][column_position("c")], Value::Int64(42));
    assert_eq!(
        stale_alias.rows(),
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_delete_unique_index_storage_when_index_is_dropped() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop_unique_index_storage");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_unique_index_storage (id TEXT PRIMARY KEY, email TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_unique_index_storage (id, email) VALUES ('r1', 'a@example.com')",
            vec![],
        )
        .expect("insert unique value");
    let keys_before_index = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan before index")
        .into_iter()
        .map(|(key, _)| key)
        .collect::<HashSet<_>>();
    cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX drop_unique_index_storage_email ON drop_unique_index_storage (email)",
                vec![],
            )
            .expect("create unique index");
    let keys_after_index = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan after index")
        .into_iter()
        .map(|(key, _)| key)
        .collect::<HashSet<_>>();
    let index_keys = keys_after_index
        .difference(&keys_before_index)
        .cloned()
        .collect::<HashSet<_>>();
    assert!(
        !index_keys.is_empty(),
        "unique index should create storage entries"
    );

    // Act
    cassie
        .execute_sql(
            &session,
            "DROP INDEX drop_unique_index_storage_email ON drop_unique_index_storage",
            vec![],
        )
        .expect("drop unique index");
    let keys_after_drop = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan after drop")
        .into_iter()
        .map(|(key, _)| key)
        .collect::<HashSet<_>>();

    // Assert
    assert!(
        index_keys.is_disjoint(&keys_after_drop),
        "unique index storage entries remained after DROP INDEX: {:x?}",
        index_keys
            .intersection(&keys_after_drop)
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_not_restore_dropped_column_values_after_readding_the_name() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("dropped_column_value_resurrection");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE dropped_column_values (id TEXT PRIMARY KEY, note TEXT, renamed_source TEXT)",
            "INSERT INTO dropped_column_values (id, note, renamed_source) VALUES ('r1', 'secret', 'renamed secret')",
            "ALTER TABLE dropped_column_values DROP COLUMN note",
            "ALTER TABLE dropped_column_values ADD COLUMN note TEXT",
            "ALTER TABLE dropped_column_values RENAME COLUMN renamed_source TO renamed_current",
            "ALTER TABLE dropped_column_values DROP COLUMN renamed_current",
            "ALTER TABLE dropped_column_values ADD COLUMN renamed_source TEXT",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

    // Act
    let explicit = cassie
        .execute_sql(
            &session,
            "SELECT id, note, renamed_source FROM dropped_column_values",
            vec![],
        )
        .expect("select re-added columns");
    let wildcard = cassie
        .execute_sql(&session, "SELECT * FROM dropped_column_values", vec![])
        .expect("select all re-added columns");
    let predicate = cassie
            .execute_sql(
                &session,
                "SELECT id FROM dropped_column_values WHERE note = 'secret' OR renamed_source = 'renamed secret'",
                vec![],
            )
            .expect("filter re-added columns");

    // Assert
    assert_eq!(
        explicit.rows(),
        vec![vec![
            Value::String("r1".to_string()),
            Value::Null,
            Value::Null,
        ]]
    );
    assert_eq!(wildcard.rows(), explicit.rows());
    assert_eq!(
        predicate.rows(),
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );

    drop(cassie);
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    reopened.startup().expect("restart Cassie");
    let reopened_session = reopened.create_session("tester", None);
    let after_restart = reopened
        .execute_sql(
            &reopened_session,
            "SELECT id, note, renamed_source FROM dropped_column_values",
            vec![],
        )
        .expect("select re-added columns after restart");
    assert_eq!(after_restart.rows(), explicit.rows());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_backfill_float_reservations_after_adding_unique_constraint() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("alter_unique_constraint_float_backfill");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE alter_unique_constraint_float (k FLOAT, v INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO alter_unique_constraint_float (k, v) VALUES ($1, 1)",
            vec![Value::Float64(1.0)],
        )
        .expect("insert preexisting row");
    cassie
            .execute_sql(
                &session,
                "ALTER TABLE alter_unique_constraint_float ADD CONSTRAINT alter_unique_constraint_float_k_unique UNIQUE (k)",
                vec![],
            )
            .expect("add unique constraint");

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO alter_unique_constraint_float (k, v) VALUES (1, 2)",
        vec![],
    );

    // Assert
    assert!(
        duplicate.is_err(),
        "duplicate insert unexpectedly succeeded"
    );
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT k, v FROM alter_unique_constraint_float ORDER BY v",
            vec![],
        )
        .expect("read rows");
    assert_eq!(
        rows.rows(),
        vec![vec![Value::Float64(1.0), Value::Int64(1)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_backfill_timestamp_reservations_after_creating_unique_index() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("create_unique_index_timestamp_backfill");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE create_unique_index_timestamp (at TIMESTAMP, v INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO create_unique_index_timestamp (at, v) VALUES ('2024-01-01T00:00:00Z', 1)",
            vec![],
        )
        .expect("insert preexisting row");
    cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX create_unique_index_timestamp_at_uq ON create_unique_index_timestamp (at)",
                vec![],
            )
            .expect("create unique index");

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO create_unique_index_timestamp (at, v) VALUES ('2024-01-01 00:00:00', 2)",
        vec![],
    );

    // Assert
    assert!(
        duplicate.is_err(),
        "duplicate insert unexpectedly succeeded"
    );
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT at, v FROM create_unique_index_timestamp ORDER BY v",
            vec![],
        )
        .expect("read rows");
    assert_eq!(
        rows.rows(),
        vec![vec![
            Value::String("2024-01-01 00:00:00".to_string()),
            Value::Int64(1)
        ]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_unique_constraint_when_preexisting_rows_duplicate() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("alter_unique_constraint_existing_duplicates");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE alter_unique_constraint_duplicates (k FLOAT, v INT)",
        "INSERT INTO alter_unique_constraint_duplicates (k, v) VALUES (1.0, 1)",
        "INSERT INTO alter_unique_constraint_duplicates (k, v) VALUES (1, 2)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    let added = cassie.execute_sql(
            &session,
            "ALTER TABLE alter_unique_constraint_duplicates ADD CONSTRAINT alter_unique_constraint_duplicates_k_unique UNIQUE (k)",
            vec![],
        );

    // Assert
    assert!(
        added.is_err(),
        "constraint creation accepted duplicate rows"
    );
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT k, v FROM alter_unique_constraint_duplicates ORDER BY v",
            vec![],
        )
        .expect("read existing rows");
    assert_eq!(rows.rows().len(), 2);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_unique_index_when_preexisting_rows_duplicate() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("create_unique_index_existing_duplicates");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE create_unique_index_duplicates (at TIMESTAMP, v INT)",
        "INSERT INTO create_unique_index_duplicates (at, v) VALUES ('2024-01-01T00:00:00Z', 1)",
        "INSERT INTO create_unique_index_duplicates (at, v) VALUES ('2024-01-01 00:00:00', 2)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    let created = cassie.execute_sql(
            &session,
            "CREATE UNIQUE INDEX create_unique_index_duplicates_at_uq ON create_unique_index_duplicates (at)",
            vec![],
        );

    // Assert
    assert!(created.is_err(), "unique index accepted duplicate rows");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT at, v FROM create_unique_index_duplicates ORDER BY v",
            vec![],
        )
        .expect("read existing rows");
    assert_eq!(rows.rows().len(), 2);
    let _ = std::fs::remove_dir_all(path);
}
