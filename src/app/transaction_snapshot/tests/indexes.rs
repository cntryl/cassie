//! Current index descriptors must not expose prefixes absent from the RR view.

use super::{controls, fixture, Cassie, CassieSession};
use crate::types::Value;

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, Vec::new())
        .expect(sql)
        .rows()
        .to_vec()
}

#[test]
fn should_fall_back_from_scalar_indexes_created_after_the_repeatable_read_view() {
    // Arrange
    let (cassie, reader, constant) = fixture();
    let writer = cassie.create_session("tester", None);
    execute(
        &cassie,
        &writer,
        "CREATE TABLE rr_index_rows (id INT, score INT)",
    );
    execute(&cassie, &writer, "INSERT INTO rr_index_rows VALUES (1, 10)");
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&reader, &constant, &controls)
        .expect("capture");
    execute(
        &cassie,
        &writer,
        "CREATE INDEX rr_index_score ON rr_index_rows (score)",
    );
    let index = cassie
        .catalog
        .list_indexes("rr_index_rows")
        .into_iter()
        .find(|index| index.name.ends_with("rr_index_score"))
        .expect("current index");
    let parsed = crate::sql::parse_statement("SELECT id FROM rr_index_rows WHERE score = 10")
        .expect("SELECT");
    let physical = cassie
        .compile_physical_plan(parsed, Some(&reader), Some(&controls))
        .expect("current plan");

    // Act
    let rows = execute(
        &cassie,
        &reader,
        "SELECT id FROM rr_index_rows WHERE score = 10",
    );

    // Assert
    assert!(
        physical
            .read
            .selected_index
            .as_deref()
            .is_some_and(|name| name.ends_with("rr_index_score")),
        "current planner selects the otherwise eligible late index"
    );
    assert!(!reader.transaction_index_visible(&index, &cassie.catalog));
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_fall_back_from_join_indexes_created_after_the_repeatable_read_view() {
    // Arrange
    let (cassie, reader, constant) = fixture();
    let writer = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE rr_join_left (id INT)",
        "CREATE TABLE rr_join_right (id INT)",
        "INSERT INTO rr_join_left VALUES (1)",
        "INSERT INTO rr_join_right VALUES (1)",
    ] {
        execute(&cassie, &writer, sql);
    }
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&reader, &constant, &controls)
        .expect("capture");
    execute(
        &cassie,
        &writer,
        "CREATE INDEX rr_join_right_id ON rr_join_right (id)",
    );

    // Act
    let rows = execute(
        &cassie,
        &reader,
        "SELECT l.id FROM rr_join_left l JOIN rr_join_right r ON l.id = r.id LIMIT 1",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_compare_repeatable_read_index_storage_identity_without_freezing_names() {
    // Arrange
    for hydrated in [false, true] {
        let (cassie, reader, constant) = fixture();
        let writer = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rr_identity_rows (score INT)",
            "INSERT INTO rr_identity_rows VALUES (10)",
            "CREATE INDEX rr_identity_score ON rr_identity_rows (score)",
        ] {
            execute(&cassie, &writer, sql);
        }
        let logical = cassie
            .catalog
            .list_indexes("rr_identity_rows")
            .into_iter()
            .next()
            .expect("logical index");
        let persisted = cassie
            .midge
            .get_index("rr_identity_rows", &logical.name)
            .expect("persisted metadata")
            .expect("persisted index");
        if hydrated {
            cassie
                .catalog
                .unregister_index("rr_identity_rows", &logical.name);
            cassie.catalog.register_index(persisted.clone());
        }
        let controls = controls(16 * 1024 * 1024);
        cassie
            .prepare_transaction_read_view(&reader, &constant, &controls)
            .expect("capture");
        let index = cassie
            .catalog
            .list_indexes("rr_identity_rows")
            .into_iter()
            .next()
            .expect("index");
        let mut replacement = index.clone();
        replacement.set_storage_ids(
            persisted.relation_id().expect("persisted relation"),
            persisted.storage_id().expect("persisted storage") + 1,
        );
        let mut renamed = index.clone();
        renamed.name = "rr_identity_renamed".to_owned();
        let seeks_before = cassie.runtime.snapshot().read_paths.index_seek_scans;

        // Act
        let same = reader.transaction_index_visible(&index, &cassie.catalog);
        let rows = execute(
            &cassie,
            &reader,
            "SELECT score FROM rr_identity_rows WHERE score = 10",
        );
        let seeks_after = cassie.runtime.snapshot().read_paths.index_seek_scans;
        let replaced = reader.transaction_index_visible(&replacement, &cassie.catalog);
        let renamed_visible = reader.transaction_index_visible(&renamed, &cassie.catalog);
        reader.rollback_transaction();
        let outside = reader.transaction_index_visible(&replacement, &cassie.catalog);

        // Assert
        assert!(same);
        assert_eq!(index.relation_id().is_some(), hydrated);
        assert_eq!(rows, vec![vec![Value::Int64(10)]]);
        assert!(
            seeks_after > seeks_before,
            "both logical and hydrated cache shapes must use the eligible native index"
        );
        assert!(!replaced);
        assert!(!renamed_visible);
        assert!(
            outside,
            "ordinary READ COMMITTED access-path selection stays current"
        );
    }
}

#[test]
fn should_keep_repeatable_read_rows_after_index_rebuild_or_replacement() {
    // Arrange
    for replacement in [false, true] {
        let (cassie, reader, constant) = fixture();
        let writer = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rr_rebuild_rows (id INT, score INT)",
            "INSERT INTO rr_rebuild_rows VALUES (1, 10)",
            "CREATE INDEX rr_rebuild_score ON rr_rebuild_rows (score)",
        ] {
            execute(&cassie, &writer, sql);
        }
        let controls = controls(16 * 1024 * 1024);
        cassie
            .prepare_transaction_read_view(&reader, &constant, &controls)
            .expect("capture");
        execute(
            &cassie,
            &writer,
            "UPDATE rr_rebuild_rows SET score = 20 WHERE id = 1",
        );
        if replacement {
            execute(&cassie, &writer, "DROP INDEX rr_rebuild_score");
            execute(
                &cassie,
                &writer,
                "CREATE INDEX rr_rebuild_score ON rr_rebuild_rows (score)",
            );
        } else {
            execute(&cassie, &writer, "REINDEX INDEX rr_rebuild_score");
        }

        // Act
        let old = execute(
            &cassie,
            &reader,
            "SELECT id FROM rr_rebuild_rows WHERE score = 10",
        );
        let newer = execute(
            &cassie,
            &reader,
            "SELECT id FROM rr_rebuild_rows WHERE score = 20",
        );

        // Assert
        assert_eq!(old, vec![vec![Value::Int64(1)]]);
        assert_eq!(newer, Vec::<Vec<Value>>::new());
    }
}

#[test]
fn should_ignore_late_scalar_prefilters_for_retained_fulltext_candidates() {
    // Arrange
    let (cassie, reader, constant) = fixture();
    let writer = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE rr_search_rows (id INT, score INT, body TEXT)",
        "INSERT INTO rr_search_rows VALUES (1, 10, 'alpha')",
        "CREATE INDEX rr_search_body ON rr_search_rows USING fulltext (body)",
    ] {
        execute(&cassie, &writer, sql);
    }
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&reader, &constant, &controls)
        .expect("capture");
    execute(
        &cassie,
        &writer,
        "CREATE INDEX rr_search_score ON rr_search_rows (score)",
    );

    // Act
    let rows = execute(
        &cassie,
        &reader,
        "SELECT id FROM rr_search_rows WHERE search(body, 'alpha') AND score = 10",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_decline_old_indexes_for_a_recreated_relation_with_the_same_name() {
    // Arrange
    let (cassie, reader, constant) = fixture();
    let writer = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE rr_recreated_rows (id INT, score INT)",
        "INSERT INTO rr_recreated_rows VALUES (1, 10)",
        "CREATE INDEX rr_recreated_score ON rr_recreated_rows (score)",
    ] {
        execute(&cassie, &writer, sql);
    }
    let original_id = cassie
        .catalog
        .get_collection("rr_recreated_rows")
        .expect("original collection")
        .storage_id;
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&reader, &constant, &controls)
        .expect("capture");
    for sql in [
        "DROP TABLE rr_recreated_rows",
        "CREATE TABLE rr_recreated_rows (id INT, score INT)",
        "INSERT INTO rr_recreated_rows VALUES (2, 20)",
        "CREATE INDEX rr_recreated_score ON rr_recreated_rows (score)",
    ] {
        execute(&cassie, &writer, sql);
    }
    let current_id = cassie
        .catalog
        .get_collection("rr_recreated_rows")
        .expect("current collection")
        .storage_id;

    // Act
    let indexed = execute(
        &cassie,
        &reader,
        "SELECT id FROM rr_recreated_rows WHERE score = 10",
    );
    let direct = execute(&cassie, &reader, "SELECT id FROM rr_recreated_rows");

    // Assert
    assert!(original_id > 0);
    assert!(current_id > 0);
    assert_ne!(original_id, current_id);
    assert!(
        indexed.is_empty(),
        "current relation identity cannot expose the removed relation's old index rows"
    );
    assert_eq!(direct, Vec::<Vec<Value>>::new());
}

#[test]
fn should_decline_recreated_indexes_whose_read_definition_changed() {
    // Arrange
    let (cassie, reader, constant) = fixture();
    let writer = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE rr_changed_index_rows (id INT, score INT)",
        "INSERT INTO rr_changed_index_rows VALUES (1, 10)",
        "CREATE INDEX rr_changed_index ON rr_changed_index_rows (score)",
    ] {
        execute(&cassie, &writer, sql);
    }
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&reader, &constant, &controls)
        .expect("capture");
    for sql in [
        "DROP INDEX rr_changed_index",
        "CREATE INDEX rr_changed_index ON rr_changed_index_rows (id)",
    ] {
        execute(&cassie, &writer, sql);
    }

    // Act
    let rows = execute(
        &cassie,
        &reader,
        "SELECT id FROM rr_changed_index_rows WHERE id = 1",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}
