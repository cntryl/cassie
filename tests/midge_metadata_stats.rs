#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::ProjectionRebuildState;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::midge::adapter::{RowDecode, StorageFamily, StorageLayout};
use cassie::types::{DataType, FieldSchema, Schema};
use cntryl_midge::TransactionMode;
use std::path::PathBuf;
use uuid::Uuid;

fn without_fallback() {
    std::env::remove_var("CASSIE_MIDGE_ALLOW_FALLBACK");
}

fn data_dir(label: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-v1-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

fn normalize_family_ids(layout: &StorageLayout) -> (u32, u32, u32) {
    (layout.schema.id(), layout.data.id(), layout.temp.id())
}

fn put_legacy_document(cassie: &Cassie, collection: &str, id: &str, payload: serde_json::Value) {
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(
        format!("doc:{collection}:{id}").into_bytes(),
        payload.to_string().into_bytes(),
        None,
    )
    .unwrap();
    tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
}

#[test]
fn should_restore_cardinality_stats_after_restart() {
    // Arrange
    let path = data_dir("cardinality_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_cardinality_restart";
        cassie
            .midge
            .create_collection(
                collection,
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha", "body": "bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_title".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: false,
                options: Default::default(),
            })
            .unwrap();
        cassie
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .unwrap();

        // Act
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.midge.ensure_families_ready().unwrap();
        let stats = restarted
            .midge
            .get_cardinality_stats(collection)
            .unwrap()
            .expect("stored cardinality stats");

        // Assert
        assert!(stats.hydrated);
        assert_eq!(stats.row_count, 1);
        assert_eq!(
            stats
                .indexes
                .get("scalar:idx_title")
                .map(|entry| entry.cardinality),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_move_cleanup_cardinality_stats_on_collection_rename_drop() {
    // Arrange
    let path = data_dir("cardinality_cleanup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let current = "cf_layout_cardinality_cleanup";
        let next = "cf_layout_cardinality_cleanup_next";
        cassie
            .midge
            .create_collection(
                current,
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                current,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        cassie
            .midge
            .rebuild_cardinality_stats_for_collection(current)
            .unwrap();

        // Act
        cassie.midge.rename_collection(current, next).unwrap();
        let renamed_stats = cassie
            .midge
            .get_cardinality_stats(next)
            .unwrap()
            .expect("renamed stats");
        let old_stats = cassie.midge.get_cardinality_stats(current).unwrap();
        cassie.midge.drop_collection(next).unwrap();
        let dropped_stats = cassie.midge.get_cardinality_stats(next).unwrap();

        // Assert
        assert_eq!(renamed_stats.row_count, 1);
        assert!(old_stats.is_none());
        assert!(dropped_stats.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_projection_metadata_in_schema_family() {
    // Arrange
    let path = data_dir("projection_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_projection_metadata";

        // Act
        cassie
            .midge
            .create_collection(
                collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .unwrap();
        let metadata = cassie
            .midge
            .projection_metadata(collection)
            .unwrap()
            .expect("projection metadata should exist");
        let raw_entries = cassie
            .midge
            .raw_scan_prefix(
                StorageFamily::Schema,
                format!("__cassie__/projection/{collection}").as_bytes(),
            )
            .unwrap();

        // Assert
        assert_eq!(metadata.collection, collection);
        assert_eq!(metadata.schema_version, 1);
        assert_eq!(metadata.offset, 0);
        assert_eq!(metadata.lag, 0);
        assert_eq!(metadata.rebuild_state, ProjectionRebuildState::Idle);
        assert_eq!(raw_entries.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_projection_metadata_during_startup() {
    // Arrange
    let path = data_dir("projection_metadata_hydrate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        cassie
            .midge
            .create_collection(
                "hydrated_projection_metadata",
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .unwrap();

        // Act
        cassie.startup().unwrap();
        let metadata = cassie
            .catalog
            .get_projection_metadata("hydrated_projection_metadata")
            .expect("projection metadata should hydrate");

        // Assert
        assert_eq!(metadata.collection, "hydrated_projection_metadata");
        assert_eq!(metadata.schema_version, 1);
        assert_eq!(metadata.rebuild_state, ProjectionRebuildState::Idle);

        let _ = std::fs::remove_dir_all(path);
    });
}
