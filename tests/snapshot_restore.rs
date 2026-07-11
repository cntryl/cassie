#![allow(unused_imports, dead_code)]

use cassie::app::{Cassie, CassieSnapshotManifest, CassieSnapshotOptions};
use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::catalog::canonical_relation_name;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn canonical_collection(name: &str) -> String {
    canonical_relation_name("postgres", "public", name)
}

fn seed_replayed_projection(path: &str, table: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {table} (title TEXT, score INT)"),
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, table);
        cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-1".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-1".to_string(),
                    checkpoint: "checkpoint-1".to_string(),
                    position: Some(1),
                    document_id: "doc-1".to_string(),
                    payload: Some(serde_json::json!({"title": "alpha", "score": 10})),
                }],
            })
            .unwrap();
        cassie
            .execute_sql(
                &session,
                &format!("VERIFY PROJECTION {table} MODE full"),
                vec![],
            )
            .unwrap();
        drop(cassie);
    });
}

#[test]
fn should_create_snapshot_manifest_with_projection_checkpoint_hash_metadata() {
    // Arrange
    with_fallback();
    let source = data_dir("snapshot_manifest_source");
    let snapshot = data_dir("snapshot_manifest_bundle");
    seed_replayed_projection(&source, "snapshot_manifest_docs");

    // Act
    let manifest = Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(1_234),
        },
    )
    .unwrap();
    let manifest_path = std::path::Path::new(&snapshot).join("cassie-snapshot-manifest.json");
    let manifest_file: CassieSnapshotManifest =
        serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();

    // Assert
    assert_eq!(manifest, manifest_file);
    assert_eq!(manifest.format_version, 2);
    assert_eq!(manifest.cassie_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.generated_ms, 1_234);
    assert_eq!(manifest.compatibility_status, "compatible");
    assert_eq!(manifest.midge_data_path, "midge");
    assert!(manifest.schema_epoch >= 1);
    let projection = manifest
        .projections
        .iter()
        .find(|projection| {
            projection.projection_id == canonical_collection("snapshot_manifest_docs")
        })
        .expect("projection manifest");
    assert_eq!(projection.source_identity.as_deref(), Some("orders-stream"));
    assert_eq!(
        projection.source_checkpoint.as_deref(),
        Some("checkpoint-1")
    );
    assert_eq!(projection.source_position, Some(1));
    assert_eq!(projection.hash.algorithm, "cassie-fnv128");
    assert_eq!(projection.hash.digest_length, 16);
    assert!(projection.hash.root_digest.is_some());
    assert_eq!(projection.hash.root_state, "current");

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(snapshot);
}

#[test]
fn should_record_collection_generations_for_snapshot_consistency() {
    // Arrange
    with_fallback();
    let source = data_dir("snapshot_collection_generations_source");
    let snapshot = data_dir("snapshot_collection_generations_bundle");
    seed_replayed_projection(&source, "snapshot_collection_generations_docs");
    let collection = canonical_collection("snapshot_collection_generations_docs");

    // Act
    let manifest = Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(4_680),
        },
    )
    .expect("create snapshot");
    let source_cassie = Cassie::new_with_data_dir(&source).expect("reopen source");
    source_cassie.startup().expect("start source");

    // Assert
    let generation = source_cassie
        .midge
        .collection_generation(&collection)
        .expect("source generation");
    let recorded = manifest
        .collections
        .iter()
        .find(|entry| entry.collection == collection)
        .expect("collection generation manifest");
    assert_eq!(recorded.generation, generation);
    assert!(manifest.data_epoch > 0);

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(snapshot);
}

#[test]
fn should_restore_snapshot_to_new_data_dir_for_startup_query() {
    // Arrange
    with_fallback();
    let source = data_dir("snapshot_restore_source");
    let snapshot = data_dir("snapshot_restore_bundle");
    let restored = data_dir("snapshot_restore_restored");
    seed_replayed_projection(&source, "snapshot_restore_docs");
    Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(2_468),
        },
    )
    .unwrap();

    // Act
    let restored_manifest = Cassie::restore_snapshot(&snapshot, &restored).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&restored).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let projection = canonical_test_collection(&cassie, "snapshot_restore_docs");
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM snapshot_restore_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT source_checkpoint, last_applied_event_id, freshness FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(restored_manifest.generated_ms, 2_468);
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(10)]]
        );
        assert_eq!(
            checkpoint.rows,
            vec![vec![
                Value::String("checkpoint-1".to_string()),
                Value::String("event-1".to_string()),
                Value::String("fresh".to_string()),
            ]]
        );

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
        let _ = std::fs::remove_dir_all(restored);
    });
}

#[test]
fn should_reject_v1_snapshot_manifest_with_expected_v2_before_restore() {
    // Arrange
    with_fallback();
    let source = data_dir("snapshot_incompatible_source");
    let snapshot = data_dir("snapshot_incompatible_bundle");
    let restored = data_dir("snapshot_incompatible_restored");
    seed_replayed_projection(&source, "snapshot_incompatible_docs");
    Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(3_579),
        },
    )
    .unwrap();
    let manifest_path = std::path::Path::new(&snapshot).join("cassie-snapshot-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["format_version"] = serde_json::json!(1);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Act
    let error = Cassie::restore_snapshot(&snapshot, &restored).unwrap_err();

    // Assert
    assert!(error
        .to_string()
        .contains("snapshot manifest version 1 is unsupported; expected 2"));
    assert!(!std::path::Path::new(&restored).exists());

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(snapshot);
}

#[test]
fn should_reject_restore_when_manifest_epoch_does_not_match_copied_state() {
    // Arrange
    with_fallback();
    let source = data_dir("snapshot_epoch_mismatch_source");
    let snapshot = data_dir("snapshot_epoch_mismatch_bundle");
    let restored = data_dir("snapshot_epoch_mismatch_restored");
    seed_replayed_projection(&source, "snapshot_epoch_mismatch_docs");
    Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(4_691),
        },
    )
    .expect("create snapshot");
    let manifest_path = std::path::Path::new(&snapshot).join("cassie-snapshot-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["data_epoch"] = serde_json::json!(manifest["data_epoch"].as_u64().unwrap() + 1);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Act
    let error = Cassie::restore_snapshot(&snapshot, &restored).unwrap_err();

    // Assert
    assert!(error
        .to_string()
        .contains("snapshot data epoch does not match restored data"));
    assert!(!std::path::Path::new(&restored).exists());

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(snapshot);
    let _ = std::fs::remove_dir_all(restored);
}

#[cfg(unix)]
#[test]
fn should_remove_partial_snapshot_after_copy_error() {
    // Arrange
    with_fallback();
    let source = data_dir("snapshot_copy_error_source");
    let snapshot = data_dir("snapshot_copy_error_bundle");
    seed_replayed_projection(&source, "snapshot_copy_error_docs");
    let regular_file = std::path::Path::new(&source).join("aaa-copy-marker");
    std::fs::write(&regular_file, b"copy before failure").unwrap();
    let special_file = std::path::Path::new(&source).join("zzz-copy-link");
    std::os::unix::fs::symlink("missing-target", &special_file).unwrap();

    // Act
    let error = Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(5_791),
        },
    )
    .unwrap_err();

    // Assert
    assert!(error
        .to_string()
        .contains("snapshot copy does not support special file"));
    assert!(!std::path::Path::new(&snapshot).exists());

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(snapshot);
}

#[cfg(unix)]
#[test]
fn should_remove_partial_restore_after_copy_error() {
    // Arrange
    with_fallback();
    let source = data_dir("restore_copy_error_source");
    let snapshot = data_dir("restore_copy_error_bundle");
    let target = data_dir("restore_copy_error_target");
    seed_replayed_projection(&source, "restore_copy_error_docs");
    Cassie::create_snapshot_from_data_dir(
        &source,
        &snapshot,
        CassieSnapshotOptions {
            generated_ms: Some(6_802),
        },
    )
    .unwrap();
    let snapshot_midge = std::path::Path::new(&snapshot).join("midge");
    std::fs::write(
        snapshot_midge.join("aaa-copy-marker"),
        b"copy before failure",
    )
    .unwrap();
    std::os::unix::fs::symlink("missing-target", snapshot_midge.join("zzz-copy-link")).unwrap();

    // Act
    let error = Cassie::restore_snapshot(&snapshot, &target).unwrap_err();

    // Assert
    assert!(error
        .to_string()
        .contains("snapshot copy does not support special file"));
    assert!(!std::path::Path::new(&target).exists());

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(snapshot);
    let _ = std::fs::remove_dir_all(target);
}
