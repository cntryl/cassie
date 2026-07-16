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
fn should_bootstrap_required_families_idempotently() {
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
        assert_eq!(first_state.1 .0, "cf0");
        assert!(first_state.1 .1.starts_with("db-"));
        assert_eq!(first_state.1 .2, "cf1");
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
        let legacy_row_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"r/")
            .unwrap();
        let legacy_doc_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:")
            .unwrap();
        let legacy_schema_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"__cassie__/schema/")
            .unwrap();
        let schema_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .unwrap();
        let temp_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Temp, b"")
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
        assert!(legacy_schema_entries.is_empty());
        assert!(schema_entries
            .iter()
            .any(|(_, value)| value.as_slice() == b"cassie-midge-layout-v1"));
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
fn should_reject_v1_data_prefix_after_reopen() {
    // Arrange
    let path = data_dir("v1_data_reject");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();
            let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
            tx.put(b"doc:legacy:1".to_vec(), b"{}".to_vec(), None)
                .unwrap();
            tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let result = restarted.startup();

        // Assert
        let error = result.expect_err("v1 data prefix should be rejected");
        assert!(
            error
                .to_string()
                .contains("incompatible cassie-midge-layout-v1 storage layout"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_older_layout_marker() {
    // Arrange
    let path = data_dir("v4_layout_marker_reject");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();
        let marker_key = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .unwrap()
            .into_iter()
            .find_map(|(key, value)| (value.as_slice() == b"cassie-midge-layout-v1").then_some(key))
            .expect("baseline layout marker key");
        let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(marker_key, b"cassie-midge-lexkey-v4".to_vec(), None)
            .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    // Act
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    let error = restarted
        .startup()
        .expect_err("v4 layout marker should be rejected");

    // Assert
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains("found marker 'cassie-midge-lexkey-v4'"),
        "unexpected error: {diagnostic}"
    );
    assert!(
        diagnostic.contains("expected baseline marker 'cassie-midge-layout-v1'"),
        "unexpected error: {diagnostic}"
    );

    let _ = std::fs::remove_dir_all(path);
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
        assert!(layout.data.name().starts_with("db-"));
        assert_eq!(layout.temp.name(), "cf1");

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
