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
fn should_bootstrap_cf0_cf1_cf2_idempotently() {
    // Arrange
    let path = data_dir("bootstrap");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let first_state = {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let families = cassie.midge.ensure_families_ready().unwrap().clone();
            (
                (families.schema.id(), families.data.id(), families.temp.id()),
                (
                    families.schema.name().to_string(),
                    families.data.name().to_string(),
                    families.temp.name().to_string(),
                ),
            )
        };

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let reloaded = restarted.midge.ensure_families_ready().unwrap().clone();
        let second_ids = normalize_family_ids(&reloaded);

        // Assert
        assert_eq!(
            first_state.1,
            ("cf0".to_string(), "cf1".to_string(), "cf2".to_string())
        );
        assert_eq!(second_ids, first_state.0);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_route_schema_data_temp_across_families() {
    // Arrange
    let path = data_dir("routing");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let _ = cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie.midge.create_collection(collection, schema).unwrap();
        let doc_id = cassie
            .midge
            .put_document(
                collection,
                None,
                serde_json::json!({"title": "alpha", "embedding": [1.0, 2.0]}),
            )
            .unwrap();

        // Act
        let data_prefix = format!("r/{collection}/").into_bytes();
        let expected_doc_key = format!("r/{collection}/{doc_id}").into_bytes();

        let data_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, data_prefix.as_slice())
            .unwrap();
        let schema_prefix = b"__cassie__/schema/";
        let schema_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, schema_prefix)
            .unwrap();
        let temp_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Temp, b"")
            .unwrap();

        // Assert
        assert!(
            data_entries.iter().any(|(key, _)| key == &expected_doc_key),
            "row blob should be stored in cf1"
        );
        assert!(
            schema_entries
                .iter()
                .any(|(key, _)| key.starts_with(schema_prefix)),
            "schema metadata should be stored in cf0"
        );
        assert!(
            temp_entries.is_empty(),
            "temp family should start empty in bootstrap state"
        );

        let mut tx = cassie.midge.temp_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(b"temp:marker".to_vec(), b"1".to_vec(), None)
            .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();

        let after_put = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Temp, b"temp:")
            .unwrap();
        assert_eq!(after_put.len(), 1);

        cassie.midge.clear_temp_family().unwrap();
        let after_cleanup = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Temp, b"")
            .unwrap();
        assert!(after_cleanup.is_empty(), "cf2 should support cleanup");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_store_rows_as_field_id_blobs_in_data_family() {
    // Arrange
    let path = data_dir("row_blob_data");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_row_blob";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie.midge.create_collection(collection, schema).unwrap();
        let doc_id = cassie
            .midge
            .put_document(
                collection,
                None,
                serde_json::json!({"title": "alpha", "embedding": [1.0, 2.0]}),
            )
            .unwrap();

        // Act
        let row_prefix = format!("r/{collection}/").into_bytes();
        let expected_row_key = format!("r/{collection}/{doc_id}").into_bytes();
        let row_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, row_prefix.as_slice())
            .unwrap();
        let stored = cassie
            .midge
            .get_document(collection, &doc_id)
            .unwrap()
            .expect("stored row should decode");

        // Assert
        assert_eq!(stored.payload["title"], "alpha");
        let (_, row_blob) = row_entries
            .iter()
            .find(|(key, _)| key == &expected_row_key)
            .expect("row blob should be stored under row key");
        let row_blob_text = String::from_utf8_lossy(row_blob);
        assert!(
            !row_blob_text.contains("title") && !row_blob_text.contains("embedding"),
            "row blob should store field ids, not field names"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_retired_field_ids_in_row_schema_metadata() {
    // Arrange
    let path = data_dir("row_schema_ids");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_row_schema";
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
            .alter_collection_drop_column(collection, "title")
            .unwrap();

        // Act
        cassie
            .midge
            .alter_collection_add_column(
                collection,
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            )
            .unwrap();
        let row_schema_entries = cassie
            .midge
            .raw_scan_prefix(
                StorageFamily::Schema,
                format!("__cassie__/row-schema/{collection}").as_bytes(),
            )
            .unwrap();

        // Assert
        let (_, row_schema_raw) = row_schema_entries
            .first()
            .expect("row schema metadata should be persisted");
        let row_schema: serde_json::Value = serde_json::from_slice(row_schema_raw).unwrap();
        assert_eq!(row_schema["schema_version"], 3);
        assert_eq!(row_schema["next_field_id"], 4);

        let fields = row_schema["fields"].as_array().expect("fields array");
        let title = fields
            .iter()
            .find(|field| field["name"] == "title")
            .expect("title field metadata should remain retired");
        let body = fields
            .iter()
            .find(|field| field["name"] == "body")
            .expect("body field metadata should remain active");
        let status = fields
            .iter()
            .find(|field| field["name"] == "status")
            .expect("status field metadata should be added");

        assert_eq!(title["field_id"], 1);
        assert_eq!(title["retired"], true);
        assert_eq!(body["field_id"], 2);
        assert_eq!(body["retired"], false);
        assert_eq!(status["field_id"], 3);
        assert_eq!(status["retired"], false);

        let _ = std::fs::remove_dir_all(path);
    });
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
                include_fields: Vec::new(),
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
fn should_move_and_cleanup_cardinality_stats_on_collection_rename_and_drop() {
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

#[test]
fn should_scan_legacy_document_rows_after_row_blob_upgrade() {
    // Arrange
    let path = data_dir("legacy_scan");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_legacy_scan";
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
        put_legacy_document(
            &cassie,
            collection,
            "legacy-1",
            serde_json::json!({"title": "legacy"}),
        );

        // Act
        let documents = cassie.midge.scan_documents(collection).unwrap();

        // Assert
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].id, "legacy-1");
        assert_eq!(documents[0].payload["title"], "legacy");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_remove_legacy_document_key_when_overwriting_with_row_blob() {
    // Arrange
    let path = data_dir("legacy_overwrite");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_legacy_overwrite";
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
        put_legacy_document(
            &cassie,
            collection,
            "doc-1",
            serde_json::json!({"title": "old"}),
        );

        // Act
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "new"}),
            )
            .unwrap();
        cassie.midge.delete_document(collection, "doc-1").unwrap();
        let after_delete = cassie.midge.get_document(collection, "doc-1").unwrap();

        // Assert
        assert!(after_delete.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_move_legacy_document_keys_when_collection_is_renamed() {
    // Arrange
    let path = data_dir("legacy_rename");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_legacy_rename";
        let renamed = "cf_layout_legacy_renamed";
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
        put_legacy_document(
            &cassie,
            collection,
            "legacy-1",
            serde_json::json!({"title": "legacy"}),
        );

        // Act
        cassie.midge.rename_collection(collection, renamed).unwrap();
        let moved = cassie.midge.get_document(renamed, "legacy-1").unwrap();
        let old_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, format!("doc:{collection}:").as_bytes())
            .unwrap();

        // Assert
        assert_eq!(
            moved.expect("legacy document should move").payload["title"],
            "legacy"
        );
        assert!(old_entries.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_delete_legacy_document_keys_when_collection_is_dropped() {
    // Arrange
    let path = data_dir("legacy_drop");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_legacy_drop";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        put_legacy_document(
            &cassie,
            collection,
            "legacy-1",
            serde_json::json!({"title": "legacy"}),
        );

        // Act
        cassie.midge.drop_collection(collection).unwrap();
        cassie.midge.create_collection(collection, schema).unwrap();
        let resurrected = cassie.midge.get_document(collection, "legacy-1").unwrap();

        // Assert
        assert!(resurrected.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_iterate_rebuild_rows_across_storage_sources() {
    // Arrange
    let path = data_dir("row_rebuild_iter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_rebuild_iter";
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
        cassie
            .midge
            .put_document(
                collection,
                Some("dupe-1".to_string()),
                serde_json::json!({"title": "row"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("row-1".to_string()),
                serde_json::json!({"title": "fresh"}),
            )
            .unwrap();
        put_legacy_document(
            &cassie,
            collection,
            "dupe-1",
            serde_json::json!({"title": "legacy-stale"}),
        );
        put_legacy_document(
            &cassie,
            collection,
            "legacy-1",
            serde_json::json!({"title": "legacy"}),
        );

        // Act
        let rows = cassie
            .midge
            .scan_rows_for_rebuild(collection, RowDecode::Full)
            .unwrap();

        // Assert
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .find(|row| row.id == "dupe-1")
                .expect("duplicate row id")
                .payload["title"],
            "row"
        );
        assert!(rows.iter().any(|row| row.id == "legacy-1"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_active_fields_for_rebuild_rows() {
    // Arrange
    let path = data_dir("row_rebuild_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_rebuild_projection";
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
                        FieldSchema {
                            name: "status".to_string(),
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
                Some("row-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "body": "retired",
                    "status": "ready",
                }),
            )
            .unwrap();
        cassie
            .midge
            .alter_collection_drop_column(collection, "body")
            .unwrap();

        // Act
        let rows = cassie
            .midge
            .scan_rows_for_rebuild(
                collection,
                RowDecode::Projected(vec!["title".to_string(), "body".to_string()]),
            )
            .unwrap();

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload, serde_json::json!({"title": "alpha"}));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_transactions_that_include_schema_plus_data_families() {
    // Arrange
    let path = data_dir("mixed_family_reject");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        // Act
        let result = cassie.midge.begin_families_tx(
            &[StorageFamily::Schema, StorageFamily::Data],
            TransactionMode::ReadWrite,
        );

        // Assert
        assert!(result.is_err());
        let error = match result {
            Ok(_) => panic!("expected mixed-family transaction to be rejected"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("cannot open a transaction across schema and data families"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_bootstrap_via_startup_path() {
    // Arrange
    let path = data_dir("bootstrap_startup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        // Act
        cassie.startup().unwrap();
        let layout = cassie.midge.ensure_families_ready().unwrap();

        // Assert
        assert_eq!(layout.schema.name(), "cf0");
        assert_eq!(layout.data.name(), "cf1");
        assert_eq!(layout.temp.name(), "cf2");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_temp_family_during_startup() {
    // Arrange
    let path = data_dir("startup_temp_cleanup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let mut tx = cassie
            .midge
            .temp_tx(cntryl_midge::TransactionMode::ReadWrite)
            .unwrap();
        tx.put(b"temp_marker".to_vec(), b"keep-me".to_vec(), None)
            .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();

        // Act
        cassie.startup().unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Temp, b"temp_")
            .unwrap();

        // Assert
        assert_eq!(entries.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_cassie_metadata_off_default_family() {
    // Arrange
    let path = data_dir("default_family_guard");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();

        let collection = "cf_layout_default_guard";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie.midge.create_collection(collection, schema).unwrap();
        let _ = cassie
            .midge
            .put_document(
                collection,
                Some("doc-default-guard".to_string()),
                serde_json::json!({"title": "alpha", "embedding": [1.0, 2.0]}),
            )
            .unwrap();

        // Act
        let default_entries = cassie.midge.raw_scan_prefix_named("default", b"").unwrap();

        // Assert
        for (key, _) in default_entries {
            let key = String::from_utf8_lossy(&key);
            assert!(
                !key.starts_with("__cassie__/")
                    && !key.starts_with("doc:")
                    && !key.starts_with("r/"),
                "no Cassie-managed keys should be stored in default family"
            );
        }

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_make_startup_idempotent_when_reinvoked() {
    // Arrange
    without_fallback();
    let path = data_dir("startup_idempotent");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        // Act
        cassie.startup().unwrap();
        let families_first = cassie.midge.ensure_families_ready().unwrap().clone();
        cassie.startup().unwrap();
        let families_second = cassie.midge.ensure_families_ready().unwrap().clone();

        // Assert
        assert_eq!(families_first.schema.id(), families_second.schema.id());
        assert_eq!(families_first.data.id(), families_second.data.id());
        assert_eq!(families_first.temp.id(), families_second.temp.id());
        assert_eq!(families_first.schema.name(), families_second.schema.name());
        assert_eq!(families_first.data.name(), families_second.data.name());
        assert_eq!(families_first.temp.name(), families_second.temp.name());

        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Temp, b"")
            .unwrap();
        assert!(entries.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fail_startup_when_data_dir_is_not_writable_directory() {
    // Arrange
    without_fallback();
    let base_path = PathBuf::from(data_dir("invalid_parent"));
    let _ = std::fs::remove_file(&base_path);
    std::fs::write(&base_path, "locked").unwrap();
    let path = format!("{}/child", base_path.to_string_lossy());

    // Act
    let created = Cassie::new_with_data_dir(&path);

    // Assert
    assert!(created.is_err());

    let _ = std::fs::remove_file(&base_path);
}

#[test]
fn should_hydrate_from_schema_records_when_collections_index_is_missing() {
    // Arrange
    let path = data_dir("schema_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "fallback_collection";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie.midge.create_collection(collection, schema).unwrap();

        let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
        tx.delete(b"__cassie__/collections".to_vec()).unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let collections = restarted
            .catalog
            .list_collections()
            .into_iter()
            .map(|collection| collection.name)
            .collect::<Vec<_>>();

        // Assert
        assert!(collections.iter().any(|value| value == collection));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_refresh_in_memory_catalog_during_startup() {
    // Arrange
    let path = data_dir("startup_catalog_refresh");
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
                "hydrated_collection",
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .unwrap();

        cassie.register_collection(
            "ghost_collection",
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        cassie.startup().unwrap();
        let collections = cassie
            .catalog
            .list_collections()
            .into_iter()
            .map(|collection| collection.name)
            .collect::<Vec<_>>();

        // Assert
        assert!(collections
            .iter()
            .any(|value| value == "hydrated_collection"));
        assert!(!collections.iter().any(|value| value == "ghost_collection"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_namespace_catalog_from_schema_family() {
    // Arrange
    let path = data_dir("schema_namespace_hydrate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        cassie.midge.create_namespace("reporting").unwrap();

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let namespaces = restarted
            .catalog
            .list_namespaces()
            .into_iter()
            .map(|namespace| namespace.name)
            .collect::<Vec<_>>();

        // Assert
        assert!(namespaces.iter().any(|name| name == "reporting"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_renamed_namespace_catalog_from_schema_family() {
    // Arrange
    let path = data_dir("schema_namespace_rename_hydrate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        cassie.midge.create_namespace("reporting").unwrap();
        cassie
            .midge
            .rename_namespace("reporting", "reporting_archive")
            .unwrap();

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let namespaces = restarted
            .catalog
            .list_namespaces()
            .into_iter()
            .map(|namespace| namespace.name)
            .collect::<Vec<_>>();

        // Assert
        assert!(!namespaces.iter().any(|name| name == "reporting"));
        assert!(namespaces.iter().any(|name| name == "reporting_archive"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_dropped_namespace_catalog_from_schema_family() {
    // Arrange
    let path = data_dir("schema_namespace_drop_hydrate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        cassie.midge.create_namespace("reporting").unwrap();
        cassie.midge.drop_namespace("reporting").unwrap();

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let namespaces = restarted
            .catalog
            .list_namespaces()
            .into_iter()
            .map(|namespace| namespace.name)
            .collect::<Vec<_>>();

        // Assert
        assert!(!namespaces.iter().any(|name| name == "reporting"));

        let _ = std::fs::remove_dir_all(path);
    });
}
