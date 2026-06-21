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
