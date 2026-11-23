use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use super::{FieldSchema, IndexKind, IndexMeta, Midge, Schema};

#[test]
fn should_decode_pending_field_add_without_default_from_legacy_record() {
    // Arrange
    let pending = super::schema_ops::PendingFieldAdd {
        collection: "postgres.public.records".to_string(),
        field: FieldSchema {
            name: "added".to_string(),
            data_type: crate::types::DataType::Int,
            nullable: true,
        },
        default_value: Some(serde_json::json!(7)),
        target_generation: 2,
    };
    let mut legacy_record = serde_json::to_value(pending).expect("serialize field-add record");
    legacy_record
        .as_object_mut()
        .expect("field-add record is an object")
        .remove("default_value");

    // Act
    let decoded: super::schema_ops::PendingFieldAdd =
        serde_json::from_value(legacy_record).expect("decode legacy field-add record");

    // Assert
    assert_eq!(decoded.default_value, None);
    assert_eq!(decoded.target_generation, 2);
}

#[test]
fn should_recover_the_exact_recorded_legacy_timestamp_default() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_recorded_field_add_default_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_with_data_dir(&path).expect("create Midge");
    midge
        .create_collection(
            "recorded_field_add_default",
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "keep".to_owned(),
                        data_type: crate::types::DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "added".to_owned(),
                        data_type: crate::types::DataType::Text,
                        nullable: true,
                    },
                ],
            },
        )
        .expect("create durable schema");
    let collection = midge.canonical_collection_name("recorded_field_add_default");
    midge
        .put_document(
            &collection,
            Some("row-1".to_owned()),
            serde_json::json!({"keep": "alpha"}),
        )
        .expect("seed row before backfill");
    let recorded_default = "2000-01-02T03:04:05.123456Z";
    let pending = super::schema_ops::PendingFieldAdd {
        collection: collection.clone(),
        field: FieldSchema {
            name: "added".to_owned(),
            data_type: crate::types::DataType::Text,
            nullable: true,
        },
        default_value: Some(serde_json::json!(recorded_default)),
        target_generation: midge
            .collection_generation(&collection)
            .expect("read current generation")
            .saturating_add(1),
    };
    let mut tx = midge
        .begin_data_rw_tx_for(&collection)
        .expect("open pending-record transaction");
    tx.put(
        Midge::field_add_operation_key(&collection, "added"),
        serde_json::to_vec(&pending).expect("encode pending field add"),
        None,
    )
    .expect("write legacy pending record");
    tx.commit(midge.write_options_sync())
        .expect("commit pending record");

    // Act
    let resumed = midge
        .resume_pending_field_add(&collection, "added")
        .expect("resume recorded field add");
    let rows = midge
        .scan_documents(&collection)
        .expect("read backfilled row");

    // Assert
    assert!(resumed);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].payload["added"].as_str(), Some(recorded_default));
    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}

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

#[test]
fn should_serialize_collection_rename_with_collection_writes() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_collection_write_gate_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Arc::new(Midge::new_with_data_dir(&path).expect("create Midge"));
    midge
        .create_collection(
            "collection_rename_gate",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    let collection = midge.canonical_collection_name("collection_rename_gate");
    let gate = midge.collection_write_gate(&collection);
    let write_guard = gate.lock();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_midge = Arc::clone(&midge);

    let worker = thread::spawn(move || {
        started_tx.send(()).expect("signal collection rename start");
        let result =
            worker_midge.rename_collection("collection_rename_gate", "collection_rename_gate_next");
        completed_tx
            .send(result)
            .expect("signal collection rename finish");
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("collection rename should start");

    // Act
    let early_result = completed_rx.recv_timeout(Duration::from_millis(100)).ok();
    let blocked_while_write_guard_held = early_result.is_none();
    drop(write_guard);
    let result = early_result.unwrap_or_else(|| {
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("collection rename should finish after the write gate is released")
    });
    worker.join().expect("join collection rename");

    // Assert
    assert!(blocked_while_write_guard_held);
    assert!(result.is_ok());
    assert!(midge.collection_schema("collection_rename_gate").is_none());
    assert!(midge
        .collection_schema("collection_rename_gate_next")
        .is_some());

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_serialize_collection_drop_with_collection_writes() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_collection_drop_gate_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Arc::new(Midge::new_with_data_dir(&path).expect("create Midge"));
    midge
        .create_collection(
            "collection_drop_gate",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    let collection = midge.canonical_collection_name("collection_drop_gate");
    let gate = midge.collection_write_gate(&collection);
    let write_guard = gate.lock();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_midge = Arc::clone(&midge);

    let worker = thread::spawn(move || {
        started_tx.send(()).expect("signal collection drop start");
        let result = worker_midge.drop_collection("collection_drop_gate");
        completed_tx
            .send(result)
            .expect("signal collection drop finish");
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("collection drop should start");

    // Act
    let early_result = completed_rx.recv_timeout(Duration::from_millis(100)).ok();
    let blocked_while_write_guard_held = early_result.is_none();
    drop(write_guard);
    let result = early_result.unwrap_or_else(|| {
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("collection drop should finish after the write gate is released")
    });
    worker.join().expect("join collection drop");

    // Assert
    assert!(blocked_while_write_guard_held);
    assert!(result.is_ok());
    assert!(midge.collection_schema("collection_drop_gate").is_none());

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_serialize_index_drop_with_collection_writes() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie_index_drop_gate_{}", uuid::Uuid::new_v4()));
    let midge = Arc::new(Midge::new_with_data_dir(&path).expect("create Midge"));
    midge
        .create_collection(
            "index_drop_gate",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    midge
        .put_document(
            "index_drop_gate",
            Some("row-1".to_string()),
            serde_json::json!({"value": "alpha"}),
        )
        .expect("seed document");
    midge
        .put_index(&IndexMeta {
            definition_semantics_version: 0,
            collection: "index_drop_gate".to_string(),
            name: "index_drop_gate_value".to_string(),
            field: "value".to_string(),
            fields: vec!["value".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        })
        .expect("create scalar index");
    let collection = midge.canonical_collection_name("index_drop_gate");
    let gate = midge.collection_write_gate(&collection);
    let write_guard = gate.lock();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_midge = Arc::clone(&midge);

    let worker = thread::spawn(move || {
        started_tx.send(()).expect("signal index drop start");
        let result = worker_midge.delete_index("index_drop_gate", "index_drop_gate_value");
        completed_tx.send(result).expect("signal index drop finish");
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("index drop should start");

    // Act
    let early_result = completed_rx.recv_timeout(Duration::from_millis(100)).ok();
    let blocked_while_write_guard_held = early_result.is_none();
    drop(write_guard);
    let result = early_result.unwrap_or_else(|| {
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("index drop should finish after the write gate is released")
    });
    worker.join().expect("join index drop");

    // Assert
    assert!(blocked_while_write_guard_held);
    assert!(result.is_ok());
    assert!(midge
        .get_index("index_drop_gate", "index_drop_gate_value")
        .expect("read dropped index")
        .is_none());

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}
