use cassie::app::Cassie;
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    set_field_rename_failure_point, DocumentWriteFailurePoint,
};
use cassie::types::Value;

use super::support_sql as support;

#[test]
fn should_allow_duplicate_keys_outside_partial_unique_index_predicate() {
    // Arrange
    let path = support::data_dir("partial_unique_index_predicate");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE partial_unique_active (email TEXT, active BOOLEAN)",
            "CREATE UNIQUE INDEX partial_unique_active_email ON partial_unique_active (email) WHERE active",
            "INSERT INTO partial_unique_active (email, active) VALUES ('a@x', true)",
            "CREATE TABLE partial_unique_inactive (email TEXT, active BOOLEAN)",
            "CREATE UNIQUE INDEX partial_unique_inactive_email ON partial_unique_inactive (email) WHERE active",
            "INSERT INTO partial_unique_inactive (email, active) VALUES ('b@x', false)",
            "INSERT INTO partial_unique_inactive (email, active) VALUES ('b@x', false)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

    cassie
        .execute_sql(
            &session,
            "CREATE TABLE partial_unique_backfill (email TEXT, active BOOLEAN)",
            vec![],
        )
        .expect("create table for partial unique index backfill");
    for _ in 0..2 {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO partial_unique_backfill (email, active) VALUES ('c@x', false)",
                vec![],
            )
            .expect("insert duplicate rows outside future index predicate");
    }

    // Act
    let inactive_with_active_key = cassie.execute_sql(
        &session,
        "INSERT INTO partial_unique_active (email, active) VALUES ('a@x', false)",
        vec![],
    );
    let repeated_inactive_key = cassie.execute_sql(
        &session,
        "INSERT INTO partial_unique_inactive (email, active) VALUES ('b@x', false)",
        vec![],
    );
    let duplicate_active_key = cassie.execute_sql(
        &session,
        "INSERT INTO partial_unique_active (email, active) VALUES ('a@x', true)",
        vec![],
    );
    let activate_duplicate_key = cassie.execute_sql(
        &session,
        "UPDATE partial_unique_active SET active = true WHERE active = false",
        vec![],
    );
    let active_rows = cassie
        .execute_sql(&session, "SELECT email FROM partial_unique_active", vec![])
        .expect("read rows with one active match");
    let inactive_rows = cassie
        .execute_sql(
            &session,
            "SELECT email FROM partial_unique_inactive",
            vec![],
        )
        .expect("read rows outside predicate");
    let partial_index_creation = cassie.execute_sql(
            &session,
            "CREATE UNIQUE INDEX partial_unique_backfill_email ON partial_unique_backfill (email) WHERE active",
            vec![],
        );

    // Assert
    assert!(
        inactive_with_active_key.is_ok(),
        "a row outside the predicate may share a key with an indexed row"
    );
    assert!(
        repeated_inactive_key.is_ok(),
        "rows outside the predicate may repeat a key"
    );
    assert!(
        duplicate_active_key.is_err(),
        "rows matching the predicate must still enforce uniqueness"
    );
    assert!(
        activate_duplicate_key.is_err(),
        "an update entering the predicate must enforce uniqueness"
    );
    assert_eq!(active_rows.rows().len(), 2);
    assert_eq!(inactive_rows.rows().len(), 3);
    assert!(
        partial_index_creation.is_ok(),
        "index backfill ignores duplicate rows outside its predicate"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allow_distinct_unique_columns_with_colliding_legacy_key_bytes() {
    // Arrange
    let path = support::data_dir("unique_reservation_field_value_collision");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_reservation_field_value_collision (id TEXT PRIMARY KEY, a BIGINT UNIQUE, a0ab TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");
    cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_reservation_field_value_collision (id, a) VALUES ('r1', -2206112389093773006)",
                vec![],
            )
            .expect("insert first unique value");

    // Act
    let inserted = cassie.execute_sql(
        &session,
        "INSERT INTO unique_reservation_field_value_collision (id, a0ab) VALUES ('r2', 'xyz12')",
        vec![],
    );

    // Assert
    assert!(inserted.is_ok(), "distinct unique columns must not collide");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_whole_number_float_duplicates_in_unique_reservations() {
    // Arrange
    let path = support::data_dir("unique_reservation_whole_float");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE unique_reservation_whole_float (price FLOAT UNIQUE)",
        "INSERT INTO unique_reservation_whole_float (price) VALUES (5.0)",
        "INSERT INTO unique_reservation_whole_float (price) VALUES (7)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    let integer_duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO unique_reservation_whole_float (price) VALUES (5)",
        vec![],
    );
    let float_duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO unique_reservation_whole_float (price) VALUES (7.0)",
        vec![],
    );

    // Assert
    assert!(integer_duplicate.is_err(), "{integer_duplicate:?}");
    assert!(float_duplicate.is_err(), "{float_duplicate:?}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_unique_reservation_when_document_is_deleted() {
    // Arrange
    let path = support::data_dir("unique_reservation_delete");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE unique_reservation_delete (email TEXT UNIQUE)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO unique_reservation_delete (email) VALUES ('reuse@example.com')",
            vec![],
        )
        .expect("insert original row");

    // Act
    cassie
        .execute_sql(
            &session,
            "DELETE FROM unique_reservation_delete WHERE email = 'reuse@example.com'",
            vec![],
        )
        .expect("delete original row");
    let inserted = cassie
        .execute_sql(
            &session,
            "INSERT INTO unique_reservation_delete (email) VALUES ('reuse@example.com')",
            vec![],
        )
        .expect("reuse unique value");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT email FROM unique_reservation_delete",
            vec![],
        )
        .expect("select rows");

    // Assert
    assert_eq!(inserted.command(), "INSERT 0 1");
    assert_eq!(
        rows.rows(),
        vec![vec![Value::String("reuse@example.com".to_string())]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_commit_transactional_unique_value_reuse_after_delete() {
    // Arrange
    let path = support::data_dir("unique_reservation_transaction_delete_insert");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE unique_reservation_transaction_delete_insert (email TEXT UNIQUE)",
            vec![],
        )
        .expect("create table");
    cassie
        .midge
        .put_document(
            "postgres.public.unique_reservation_transaction_delete_insert",
            Some("ffffffff-ffff-4fff-bfff-ffffffffffff".to_string()),
            serde_json::json!({"email": "reuse@example.com"}),
        )
        .expect("seed row with lexicographically maximal generated-ID shape");

    // Act
    for sql in [
            "BEGIN",
            "DELETE FROM unique_reservation_transaction_delete_insert WHERE email = 'reuse@example.com'",
            "INSERT INTO unique_reservation_transaction_delete_insert (email) VALUES ('reuse@example.com')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);

    // Assert
    assert!(commit.is_ok(), "transactional value reuse failed");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT email FROM unique_reservation_transaction_delete_insert",
            vec![],
        )
        .expect("read committed row");
    assert_eq!(
        rows.rows(),
        vec![vec![Value::String("reuse@example.com".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_commit_transactional_unique_value_rotation_across_updates() {
    // Arrange
    let path = support::data_dir("unique_reservation_transaction_rotation");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE unique_rotation (id INT PRIMARY KEY, code TEXT UNIQUE)",
        "INSERT INTO unique_rotation (id, code) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    for sql in [
        "BEGIN",
        "UPDATE unique_rotation SET code = 'tmp' WHERE id = 1",
        "UPDATE unique_rotation SET code = 'a' WHERE id = 2",
        "UPDATE unique_rotation SET code = 'b' WHERE id = 1",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    let rows_in_transaction = cassie
        .execute_sql(
            &session,
            "SELECT id, code FROM unique_rotation ORDER BY id",
            vec![],
        )
        .expect("read final transaction state");
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    assert!(commit.is_ok(), "valid unique rotation must commit");
    let duplicate_b = cassie.execute_sql(
        &session,
        "UPDATE unique_rotation SET code = 'b' WHERE id = 3",
        vec![],
    );
    let duplicate_a = cassie.execute_sql(
        &session,
        "UPDATE unique_rotation SET code = 'a' WHERE id = 3",
        vec![],
    );
    let rows_after_commit = cassie
        .execute_sql(
            &session,
            "SELECT id, code FROM unique_rotation ORDER BY id",
            vec![],
        )
        .expect("read committed rows");

    // Assert
    assert_eq!(
        rows_in_transaction.rows(),
        vec![
            vec![Value::Int64(1), Value::String("b".to_string())],
            vec![Value::Int64(2), Value::String("a".to_string())],
            vec![Value::Int64(3), Value::String("c".to_string())],
        ]
    );
    assert!(
        duplicate_b.is_err(),
        "rotation must retain the 'b' reservation"
    );
    assert!(
        duplicate_a.is_err(),
        "rotation must retain the 'a' reservation"
    );
    assert_eq!(rows_after_commit.rows(), rows_in_transaction.rows());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_commit_transactional_unique_index_value_rotation_across_updates() {
    // Arrange
    let path = support::data_dir("unique_index_transaction_rotation");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE unique_index_rotation (id INT PRIMARY KEY, code TEXT)",
        "CREATE UNIQUE INDEX unique_index_rotation_code ON unique_index_rotation (code)",
        "INSERT INTO unique_index_rotation (id, code) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    for sql in [
        "BEGIN",
        "UPDATE unique_index_rotation SET code = 'tmp' WHERE id = 1",
        "UPDATE unique_index_rotation SET code = 'a' WHERE id = 2",
        "UPDATE unique_index_rotation SET code = 'b' WHERE id = 1",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    let rows_in_transaction = cassie
        .execute_sql(
            &session,
            "SELECT id, code FROM unique_index_rotation ORDER BY id",
            vec![],
        )
        .expect("read final transaction state");
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    assert!(commit.is_ok(), "valid unique-index rotation must commit");
    let duplicate_b = cassie.execute_sql(
        &session,
        "UPDATE unique_index_rotation SET code = 'b' WHERE id = 3",
        vec![],
    );
    let duplicate_a = cassie.execute_sql(
        &session,
        "UPDATE unique_index_rotation SET code = 'a' WHERE id = 3",
        vec![],
    );
    let rows_after_commit = cassie
        .execute_sql(
            &session,
            "SELECT id, code FROM unique_index_rotation ORDER BY id",
            vec![],
        )
        .expect("read committed rows");

    // Assert
    assert_eq!(
        rows_in_transaction.rows(),
        vec![
            vec![Value::Int64(1), Value::String("b".to_string())],
            vec![Value::Int64(2), Value::String("a".to_string())],
            vec![Value::Int64(3), Value::String("c".to_string())],
        ]
    );
    assert!(
        duplicate_b.is_err(),
        "rotation must retain the 'b' index reservation"
    );
    assert!(
        duplicate_a.is_err(),
        "rotation must retain the 'a' index reservation"
    );
    assert_eq!(rows_after_commit.rows(), rows_in_transaction.rows());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rollback_unique_reservations_when_transaction_rotation_commit_fails() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    let path = support::data_dir("unique_reservation_transaction_rotation_failure");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE unique_rotation_failure (id INT PRIMARY KEY, code TEXT UNIQUE)",
        "INSERT INTO unique_rotation_failure (id, code) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
        "BEGIN",
        "UPDATE unique_rotation_failure SET code = 'tmp' WHERE id = 1",
        "UPDATE unique_rotation_failure SET code = 'a' WHERE id = 2",
        "UPDATE unique_rotation_failure SET code = 'b' WHERE id = 1",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    set_document_write_failure_point(Some(DocumentWriteFailurePoint::Row));
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    set_document_write_failure_point(None);
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback failed commit");
    let duplicate = cassie.execute_sql(
        &session,
        "UPDATE unique_rotation_failure SET code = 'b' WHERE id = 3",
        vec![],
    );
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, code FROM unique_rotation_failure ORDER BY id",
            vec![],
        )
        .expect("read rows after rollback");

    // Assert
    assert!(commit.is_err(), "the row failpoint must reject COMMIT");
    assert!(
        duplicate.is_err(),
        "rollback must preserve unique reservations"
    );
    assert_eq!(
        rows.rows(),
        vec![
            vec![Value::Int64(1), Value::String("a".to_string())],
            vec![Value::Int64(2), Value::String("b".to_string())],
            vec![Value::Int64(3), Value::String("c".to_string())],
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_unique_reservation_when_unique_column_is_dropped() {
    // Arrange
    let path = support::data_dir("unique_reservation_drop_column");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE unique_reservation_drop_column (id TEXT PRIMARY KEY, email TEXT UNIQUE, email_archive TEXT UNIQUE)",
            "INSERT INTO unique_reservation_drop_column (id, email, email_archive) VALUES ('r1', 'reuse@example.com', 'archive@example.com')",
            "ALTER TABLE unique_reservation_drop_column DROP COLUMN email",
            "ALTER TABLE unique_reservation_drop_column ADD COLUMN email TEXT UNIQUE",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

    // Act
    let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_drop_column (id, email, email_archive) VALUES ('r2', 'reuse@example.com', 'new-archive@example.com')",
            vec![],
        );

    // Assert
    assert!(
        inserted.is_ok(),
        "expected the dropped-column value to be reusable"
    );
    let archived_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_drop_column (id, email, email_archive) VALUES ('r3', 'another@example.com', 'archive@example.com')",
            vec![],
        );
    assert!(
        archived_duplicate.is_err(),
        "expected the sibling UNIQUE value to remain reserved"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_unique_index_reservations_when_index_is_dropped() {
    // Arrange
    let path = support::data_dir("unique_index_reservation_drop");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE unique_index_reservation_drop (id TEXT PRIMARY KEY, email TEXT)",
            "INSERT INTO unique_index_reservation_drop (id, email) VALUES ('r1', 'original@example.com')",
            "CREATE UNIQUE INDEX email_idx ON unique_index_reservation_drop (email)",
            "DROP INDEX email_idx ON unique_index_reservation_drop",
            "UPDATE unique_index_reservation_drop SET email = 'changed@example.com' WHERE id = 'r1'",
            "CREATE UNIQUE INDEX email_idx ON unique_index_reservation_drop (email)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

    // Act
    let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO unique_index_reservation_drop (id, email) VALUES ('r2', 'original@example.com')",
            vec![],
        );

    // Assert
    assert!(
        inserted.is_ok(),
        "expected the dropped-index value to be reusable"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_duplicate_unique_value_after_column_rename() {
    // Arrange
    let path = support::data_dir("unique_reservation_rename_column");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE unique_reservation_rename_column (k FLOAT UNIQUE, v INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO unique_reservation_rename_column (k, v) VALUES ($1, 1)",
            vec![Value::Float64(1.0)],
        )
        .expect("insert original row");
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE unique_reservation_rename_column RENAME COLUMN k TO kk",
            vec![],
        )
        .expect("rename unique column");

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO unique_reservation_rename_column (kk, v) VALUES (1, 2)",
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
            "SELECT kk, v FROM unique_reservation_rename_column ORDER BY v",
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
fn should_replay_unique_reservations_after_interrupted_column_rename() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("unique_reservation_rename_column_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE unique_reservation_rename_recovery (k FLOAT UNIQUE, v INT)",
        "INSERT INTO unique_reservation_rename_recovery (k, v) VALUES (1.0, 1)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    set_field_rename_failure_point(true);

    // Act
    let interrupted = cassie.execute_sql(
        &session,
        "ALTER TABLE unique_reservation_rename_recovery RENAME COLUMN k TO kk",
        vec![],
    );
    drop(cassie);
    let recovered = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    recovered.startup().expect("replay field rename");
    let session = recovered.create_session("tester", None);
    let duplicate = recovered.execute_sql(
        &session,
        "INSERT INTO unique_reservation_rename_recovery (kk, v) VALUES (1, 2)",
        vec![],
    );

    // Assert
    assert!(
        interrupted.is_err(),
        "failure point did not interrupt rename"
    );
    assert!(
        duplicate.is_err(),
        "recovered rename lost unique reservation"
    );
    let rows = recovered
        .execute_sql(
            &session,
            "SELECT kk, v FROM unique_reservation_rename_recovery ORDER BY v",
            vec![],
        )
        .expect("read rows");
    assert_eq!(
        rows.rows(),
        vec![vec![Value::Float64(1.0), Value::Int64(1)]]
    );
    let _ = std::fs::remove_dir_all(path);
}
