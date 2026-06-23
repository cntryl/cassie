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
        let legacy_row_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"r/")
            .unwrap();
        let legacy_doc_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:")
            .unwrap();
        let stored = cassie
            .midge
            .get_document(collection, &doc_id)
            .unwrap()
            .expect("stored row should decode");

        // Assert
        assert_eq!(stored.payload["title"], "alpha");
        assert!(legacy_row_entries.is_empty());
        assert!(legacy_doc_entries.is_empty());

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
        let schema_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .unwrap();

        // Assert
        let row_schema = schema_entries
            .iter()
            .filter_map(|(_, raw)| serde_json::from_slice::<serde_json::Value>(raw).ok())
            .find(|value| value["next_field_id"] == 4 && value["schema_version"] == 3)
            .expect("row schema metadata should be persisted");
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
fn should_ignore_legacy_document_rows_after_layout_break() {
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
        assert!(documents.is_empty());

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
        // Act
        let rows = cassie
            .midge
            .scan_rows_for_rebuild(collection, RowDecode::Full)
            .unwrap();

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .find(|row| row.id == "dupe-1")
                .expect("duplicate row id")
                .payload["title"],
            "row"
        );
        assert!(rows.iter().any(|row| row.id == "row-1"));

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
