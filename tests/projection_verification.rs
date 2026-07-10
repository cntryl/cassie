#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::ProjectionVerificationState;
use cassie::midge::adapter::{RowHashRecord, StorageFamily};
use cassie::sql::ast::{ProjectionVerificationMode, QueryStatement};
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_verify_projection_command() {
    // Arrange
    let sql = "VERIFY PROJECTION projection_docs VERSION v2 MODE hashes-only";

    // Act
    let parsed = cassie::sql::parse_statement(sql).unwrap();

    // Assert
    let QueryStatement::VerifyProjection(statement) = parsed.statement else {
        panic!("expected VERIFY PROJECTION");
    };
    assert_eq!(statement.name, "projection_docs");
    assert_eq!(statement.version_id.as_deref(), Some("v2"));
    assert_eq!(statement.mode, ProjectionVerificationMode::HashesOnly);
}

#[test]
fn should_keep_row_hashes_deterministic_across_restart_schema_epoch_changes() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_row_hash_deterministic");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_hash_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_hash_docs (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "projection_hash_docs");
        let row_id = cassie.midge.scan_documents(&collection).unwrap()[0]
            .id
            .clone();
        let first = cassie
            .midge
            .row_hash(&collection, &row_id)
            .unwrap()
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_hash = restarted
            .midge
            .row_hash(&collection, &row_id)
            .unwrap()
            .unwrap();

        // Act
        restarted
            .execute_sql(
                &restarted.create_session("tester", None),
                "ALTER TABLE projection_hash_docs ADD COLUMN summary TEXT",
                vec![],
            )
            .unwrap();
        let schema_hash = restarted
            .midge
            .row_hash(&collection, &row_id)
            .unwrap()
            .unwrap();
        restarted
            .execute_sql(
                &restarted.create_session("tester", None),
                "UPDATE projection_hash_docs SET title = 'beta'",
                vec![],
            )
            .unwrap();
        let updated_hash = restarted
            .midge
            .row_hash(&collection, &row_id)
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(first.digest, restarted_hash.digest);
        assert_ne!(first.digest, schema_hash.digest);
        assert_ne!(schema_hash.digest, updated_hash.digest);
        assert_eq!(updated_hash.algorithm, "cassie-fnv128");
        assert_eq!(updated_hash.digest_length, 16);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_empty_projection_root_after_delete() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_row_hash_delete");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_delete_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_delete_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let row_id = cassie
            .midge
            .scan_documents(&canonical_test_collection(
                &cassie,
                "projection_delete_docs",
            ))
            .unwrap()[0]
            .id
            .clone();
        let collection = canonical_test_collection(&cassie, "projection_delete_docs");

        // Act
        cassie
            .execute_sql(&session, "DELETE FROM projection_delete_docs", vec![])
            .unwrap();
        let row_hash = cassie.midge.row_hash(&collection, &row_id).unwrap();
        let root = cassie.midge.root_hash(&collection).unwrap().unwrap();

        // Assert
        assert!(row_hash.is_none());
        assert_eq!(root.row_count, 0);
        assert_eq!(root.state, cassie::midge::adapter::StoredHashState::Empty);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_expose_projection_verification_state_through_catalog_views() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_hash_catalog_views");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE projection_view_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_view_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_view_docs");

        // Act
        let verified = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION projection_view_docs MODE full",
                vec![],
            )
            .unwrap();
        let hashes = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT row_state, row_count, range_count, root_state FROM pg_catalog.pg_projection_hashes WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        let operations = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, verification_state, root_state FROM pg_catalog.pg_projection_operations WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        let reports = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, mode, mismatch_count, missing_count, stale_count FROM pg_catalog.pg_projection_integrity_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(verified.rows[0][0], Value::String("verified".to_string()));
        assert_eq!(
            hashes.rows,
            vec![vec![
                Value::String("current".to_string()),
                Value::Int64(1),
                Value::Int64(1),
                Value::String("current".to_string()),
            ]]
        );
        assert_eq!(
            operations.rows,
            vec![vec![
                Value::String("unknown".to_string()),
                Value::String("unknown".to_string()),
                Value::String("current".to_string()),
            ]]
        );
        assert_eq!(
            reports.rows,
            vec![vec![
                Value::String("verified".to_string()),
                Value::String("full".to_string()),
                Value::Int64(0),
                Value::Int64(0),
                Value::Int64(0),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_integrity_failure_for_corrupt_row_hash() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_hash_corruption");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_corrupt_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_corrupt_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "projection_corrupt_docs");
        let mut row_hash = cassie.midge.list_row_hashes(&collection).unwrap()[0].clone();
        let row_hash_key = row_hash_storage_key(&cassie, &collection, &row_hash.row_id);
        row_hash.digest = "00000000000000000000000000000000".to_string();
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(row_hash_key, serde_json::to_vec(&row_hash).unwrap(), None)
            .unwrap();
        tx.commit(WriteOptions::sync()).unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION projection_corrupt_docs MODE hashes_only",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows[0][0], Value::String("failed".to_string()));
        let Value::Int64(mismatches) = result.rows[0][3] else {
            panic!("expected mismatch count");
        };
        assert!(mismatches >= 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

fn row_hash_storage_key(cassie: &Cassie, collection: &str, row_id: &str) -> Vec<u8> {
    let collection = canonical_test_collection(cassie, collection);
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap()
        .into_iter()
        .find_map(|(key, value)| {
            let record = serde_json::from_slice::<RowHashRecord>(&value).ok()?;
            (record.collection == collection && record.row_id == row_id).then_some(key)
        })
        .expect("row hash key should exist")
}

#[test]
fn should_block_unverified_projection_version_activation_without_unsafe_override() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_activation_verification");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_source_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_verified AS SELECT title FROM projection_source_docs",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_verified BUILD VERSION",
                vec![],
            )
            .unwrap();
        let mut metadata = cassie
            .catalog
            .get_materialized_projection("projection_verified")
            .unwrap();
        let target = metadata
            .versions
            .iter_mut()
            .find(|version| version.version_id == "v2")
            .unwrap();
        target.verification.state = ProjectionVerificationState::Failed;
        target.verification.failure_reason = Some("test corruption".to_string());
        cassie.midge.put_projection_metadata(&metadata).unwrap();
        cassie.catalog.register_projection_metadata(metadata);

        // Act
        let blocked = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_verified ACTIVATE VERSION v2",
            vec![],
        );
        let unsafe_result = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_verified ACTIVATE VERSION v2 UNSAFE",
            vec![],
        );

        // Assert
        assert!(blocked.is_err());
        assert!(unsafe_result.is_ok());

        let _ = std::fs::remove_dir_all(path);
    });
}
