use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema};
use cntryl_midge::{TransactionMode, WriteOptions};
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-v1-break-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

fn put_legacy_data_key(path: &str, key: &[u8]) {
    let cassie = Cassie::new_with_data_dir(path).unwrap();
    cassie.midge.ensure_families_ready().unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(key.to_vec(), b"{}".to_vec(), None).unwrap();
    tx.commit(WriteOptions::sync()).unwrap();
}

#[test]
fn should_reject_legacy_doc_prefix_on_reopen() {
    // Arrange
    let path = data_dir("doc_prefix");
    put_legacy_data_key(&path, b"doc:legacy:1");

    // Act
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    let result = restarted.startup();

    // Assert
    let error = result.expect_err("legacy doc prefix should be rejected");
    assert!(
        error
            .to_string()
            .contains("incompatible lexkey v2 storage layout"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_legacy_row_prefix_on_reopen() {
    // Arrange
    let path = data_dir("row_prefix");
    put_legacy_data_key(&path, b"r/legacy/1");

    // Act
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    let result = restarted.startup();

    // Assert
    let error = result.expect_err("legacy row prefix should be rejected");
    assert!(
        error
            .to_string()
            .contains("incompatible lexkey v2 storage layout"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_ignore_legacy_doc_key_written_after_bootstrap() {
    // Arrange
    let path = data_dir("post_bootstrap_doc");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.midge.ensure_families_ready().unwrap();
    cassie
        .midge
        .create_collection(
            "legacy_break",
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        )
        .unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(
        b"doc:legacy_break:stale".to_vec(),
        serde_json::json!({"title": "stale"})
            .to_string()
            .into_bytes(),
        None,
    )
    .unwrap();
    tx.commit(WriteOptions::sync()).unwrap();

    // Act
    let scanned = cassie.midge.scan_documents("legacy_break").unwrap();
    let legacy_entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"doc:")
        .unwrap();

    // Assert
    assert!(scanned.is_empty());
    assert_eq!(legacy_entries.len(), 1);

    let _ = std::fs::remove_dir_all(path);
}
