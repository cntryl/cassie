#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::canonical_schema_name;
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
fn should_reject_legacy_collections_index_on_reopen() {
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

        {
            let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
            tx.put(
                b"__cassie__/collections".to_vec(),
                serde_json::to_vec(&vec![collection]).unwrap(),
                None,
            )
            .unwrap();
            tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
        }

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let result = restarted.startup();

        // Assert
        let error = result.expect_err("legacy collections index should be rejected");
        assert!(
            error
                .to_string()
                .contains("incompatible lexkey v3 storage layout"),
            "unexpected error: {error}"
        );

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
        let namespace = canonical_schema_name("postgres", "reporting");

        cassie.midge.create_namespace(&namespace).unwrap();

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
        assert!(namespaces.iter().any(|name| name == &namespace));

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
        let current = canonical_schema_name("postgres", "reporting");
        let next = canonical_schema_name("postgres", "reporting_archive");

        cassie.midge.create_namespace(&current).unwrap();
        cassie.midge.rename_namespace(&current, &next).unwrap();

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
        assert!(!namespaces.iter().any(|name| name == &current));
        assert!(namespaces.iter().any(|name| name == &next));

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
        let namespace = canonical_schema_name("postgres", "reporting");

        cassie.midge.create_namespace(&namespace).unwrap();
        cassie.midge.drop_namespace(&namespace).unwrap();

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
        assert!(!namespaces.iter().any(|name| name == &namespace));

        let _ = std::fs::remove_dir_all(path);
    });
}
