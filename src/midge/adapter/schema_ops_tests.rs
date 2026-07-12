use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use super::{FieldSchema, Midge, Schema};

#[test]
fn should_serialize_field_rename_with_collection_writes() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie_schema_write_gate_{}", uuid::Uuid::new_v4()));
    let midge = Arc::new(Midge::new_with_data_dir(&path).expect("create Midge"));
    midge
        .create_collection(
            "schema_write_gate",
            Schema {
                fields: vec![FieldSchema {
                    name: "before".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    let collection = midge.canonical_collection_name("schema_write_gate");
    let gate = midge.collection_write_gate(&collection);
    let write_guard = gate.lock();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_midge = Arc::clone(&midge);

    let worker = thread::spawn(move || {
        started_tx.send(()).expect("signal schema operation start");
        let result =
            worker_midge.alter_collection_rename_column("schema_write_gate", "before", "after");
        completed_tx
            .send(result)
            .expect("signal schema operation finish");
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("schema operation should start");

    // Act
    let early_result = completed_rx.recv_timeout(Duration::from_millis(100)).ok();
    let blocked_while_write_guard_held = early_result.is_none();
    drop(write_guard);
    let result = early_result.unwrap_or_else(|| {
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("schema operation should finish after the write gate is released")
    });
    worker.join().expect("join schema operation");

    // Assert
    assert!(blocked_while_write_guard_held);
    assert!(result.is_ok());
    assert!(midge
        .collection_schema(&collection)
        .expect("read renamed schema")
        .fields
        .iter()
        .any(|field| field.name == "after"));

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}
