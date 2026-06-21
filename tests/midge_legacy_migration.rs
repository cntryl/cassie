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
