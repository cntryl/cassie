#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{CollectionMeta, CollectionStorageMode, IndexKind, IndexMeta};
use cassie::catalog::{ProjectionFreshness, ProjectionRebuildState};
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

fn put_legacy_document(cassie: &Cassie, collection: &str, id: &str, payload: &serde_json::Value) {
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
            .put_index(&IndexMeta {
                collection: collection.to_string(),
                name: "idx_title".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: false,
                options: std::collections::BTreeMap::default(),
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
fn should_rebuild_field_cardinality_stats() {
    // Arrange
    let path = data_dir("field_cardinality");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();
        let collection = "cf_layout_field_cardinality";
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
                serde_json::json!({"title": "alpha", "body": null}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"title": "beta", "body": "two"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({"body": "three"}),
            )
            .unwrap();

        // Act
        let stats = cassie
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .unwrap();

        // Assert
        let title = stats.fields.get("title").expect("title field stats");
        assert_eq!(title.non_null_count, 2);
        assert_eq!(title.missing_count, 1);
        assert_eq!(title.distinct_count, 2);
        assert_eq!(title.min_value.as_deref(), Some("\"alpha\""));
        assert_eq!(title.max_value.as_deref(), Some("\"beta\""));
        let body = stats.fields.get("body").expect("body field stats");
        assert_eq!(body.non_null_count, 2);
        assert_eq!(body.null_count, 1);
        assert_eq!(body.distinct_count, 2);
        assert_eq!(title.sample_count, 3);
        assert_eq!(title.confidence, 66);
        assert_eq!(title.histogram_buckets.len(), 2);
        assert_eq!(title.heavy_hitters[0].value, "\"alpha\"");
        assert_eq!(title.heavy_hitters[0].count, 1);

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
fn should_cleanup_column_store_keys_after_collection_rename_then_drop() {
    // Arrange
    let path = data_dir("column_store_cleanup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();
        let collection = "cf_layout_column_store_cleanup";
        let renamed = "cf_layout_column_store_cleanup_archive";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };
        let metadata = CollectionMeta::new_with_storage_mode(
            collection,
            None,
            CollectionStorageMode::ColumnStore,
        );
        cassie
            .midge
            .create_collection_with_meta(collection, &schema, &metadata)
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha", "score": 7}),
            )
            .unwrap();

        assert!(cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"__cassie__/column-store/v1/")
            .unwrap()
            .is_empty());

        // Act
        cassie.midge.rename_collection(collection, renamed).unwrap();
        let metadata = cassie
            .midge
            .collection_metadata(renamed)
            .unwrap()
            .expect("collection metadata");
        let moved = cassie.midge.get_document(renamed, "doc-1").unwrap();
        cassie.midge.drop_collection(renamed).unwrap();

        // Assert
        assert_eq!(metadata.storage_mode, CollectionStorageMode::ColumnStore);
        assert!(moved.is_some());
        assert!(cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"__cassie__/column-store/v1/")
            .unwrap()
            .is_empty());
        assert!(cassie.midge.collection_metadata(renamed).unwrap().is_none());

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
        let legacy_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"__cassie__/projection/")
            .unwrap();

        // Assert
        assert_eq!(metadata.collection, collection);
        assert_eq!(metadata.schema_version, 1);
        assert_eq!(metadata.offset, 0);
        assert_eq!(metadata.lag, 0);
        assert_eq!(metadata.rebuild_state, ProjectionRebuildState::Idle);
        assert!(legacy_entries.is_empty());

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

#[test]
fn should_reject_legacy_projection_metadata_on_reopen() {
    // Arrange
    let path = data_dir("projection_metadata_legacy");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();
            let legacy = serde_json::json!({
                "collection": "legacy_projection_metadata",
                "schema_version": 1,
                "offset": 9,
                "lag": 2,
                "rebuild_state": "idle"
            });
            let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
            tx.put(
                b"__cassie__/projection/legacy_projection_metadata".to_vec(),
                legacy.to_string().into_bytes(),
                None,
            )
            .unwrap();
            tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let result = restarted.startup();

        // Assert
        let error = result.expect_err("legacy projection metadata should reject v2 startup");
        assert!(
            error
                .to_string()
                .contains("incompatible lexkey v4 storage layout"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cleanup_projection_checkpoint_metadata_on_rename_drop() {
    // Arrange
    let path = data_dir("projection_checkpoint_cleanup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();
        let current = "projection_checkpoint_cleanup";
        let next = "projection_checkpoint_cleanup_next";
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
        let mut metadata = cassie
            .midge
            .projection_metadata(current)
            .unwrap()
            .expect("projection metadata");
        metadata.source_identity = Some("orders-stream".to_string());
        metadata.source_checkpoint = Some("checkpoint-7".to_string());
        metadata.last_applied_event_id = Some("event-7".to_string());
        metadata.replay_batch_id = Some("batch-7".to_string());
        metadata.freshness = ProjectionFreshness::Fresh;
        cassie.midge.put_projection_metadata(&metadata).unwrap();

        // Act
        cassie.midge.rename_collection(current, next).unwrap();
        let renamed = cassie
            .midge
            .projection_metadata(next)
            .unwrap()
            .expect("renamed metadata");
        let old = cassie.midge.projection_metadata(current).unwrap();
        cassie.midge.drop_collection(next).unwrap();
        let dropped = cassie.midge.projection_metadata(next).unwrap();

        // Assert
        assert_eq!(renamed.collection, next);
        assert_eq!(renamed.source_identity.as_deref(), Some("orders-stream"));
        assert_eq!(renamed.source_checkpoint.as_deref(), Some("checkpoint-7"));
        assert_eq!(renamed.last_applied_event_id.as_deref(), Some("event-7"));
        assert_eq!(renamed.replay_batch_id.as_deref(), Some("batch-7"));
        assert_eq!(renamed.freshness, ProjectionFreshness::Fresh);
        assert!(old.is_none());
        assert!(dropped.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}
