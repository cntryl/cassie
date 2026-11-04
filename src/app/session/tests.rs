use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use crate::config::CassieRuntimeLimits;
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;

use super::{
    BTreeMap, CassieError, CassieSession, CollectionChanges, StagedWriteSnapshot,
    TransactionRowChange,
};

#[test]
fn should_include_staged_snapshot_btree_nodes_before_retaining_shared_changes() {
    // Arrange
    let snapshot = StagedWriteSnapshot {
        changes: Arc::new(BTreeMap::from([(
            "one".to_owned(),
            TransactionRowChange::Delete,
        )])),
    };
    let node_bytes = 11
        * (std::mem::size_of::<String>() + std::mem::size_of::<TransactionRowChange>())
        + 16 * std::mem::size_of::<usize>();
    let owned_container_bytes = std::mem::size_of::<CollectionChanges>()
        + 2 * std::mem::size_of::<usize>()
        + node_bytes
        + "one".len();

    // Act
    let estimated_bytes = snapshot
        .estimated_retained_bytes()
        .expect("snapshot retained estimate");

    // Assert
    assert!(
        estimated_bytes >= owned_container_bytes,
        "a retained snapshot owns BTree slots and its Arc allocation"
    );
}

#[test]
fn should_reserve_sparse_staged_snapshot_json_before_retaining_owned_changes() {
    // Arrange
    let session = CassieSession::new("postgres".to_owned(), None);
    session.begin_transaction(None).expect("begin transaction");
    let mut payload = serde_json::Value::Null;
    for _ in 0..32 {
        payload = json!({"a": payload});
    }
    session
        .stage_document_write("items", "one".to_owned(), payload)
        .expect("stage sparse JSON");
    let snapshot = session.staged_write_snapshot("items");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 16 * 1_024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let retained_calls = Cell::new(0);

    // Act
    let result = Accounted::try_new(
        &controls,
        snapshot
            .estimated_retained_bytes()
            .expect("snapshot retained estimate"),
        || {
            retained_calls.set(retained_calls.get() + 1);
            snapshot.shared_changes()
        },
    );

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        retained_calls.get(),
        0,
        "reserve the complete staged snapshot"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_staged_write_snapshot_immutable_when_session_changes() {
    // Arrange
    let session = CassieSession::new("postgres".to_string(), Some("cassie".to_string()));
    session.begin_transaction(None).expect("begin transaction");
    session
        .stage_document_write(
            "cassie.public.snapshot_items",
            "item-a".to_string(),
            json!({"value": "before"}),
        )
        .expect("stage initial row");
    let snapshot = session.staged_write_snapshot("snapshot_items");
    let shared_changes = snapshot.shared_changes();
    let retained_bytes = snapshot
        .estimated_retained_bytes()
        .expect("snapshot retained estimate");

    // Act
    session
        .stage_document_write(
            "cassie.public.snapshot_items",
            "item-b".to_string(),
            json!({"value": "after"}),
        )
        .expect("stage later row");
    session
        .stage_document_delete("cassie.public.snapshot_items", "item-a".to_string())
        .expect("stage later delete");
    let updated = session.staged_write_snapshot("snapshot_items");

    // Assert
    assert!(Arc::ptr_eq(&shared_changes, &snapshot.shared_changes()));
    assert!(retained_bytes > "item-a".len());
    assert!(matches!(
        snapshot.ordered_changes().get("item-a"),
        Some(TransactionRowChange::Upsert(payload))
            if payload == &json!({"value": "before"})
    ));
    assert!(!snapshot.ordered_changes().contains_key("item-b"));
    assert!(matches!(
        updated.ordered_changes().get("item-a"),
        Some(TransactionRowChange::Delete)
    ));
    assert!(updated.ordered_changes().contains_key("item-b"));
    assert!(session.has_collection_changes("snapshot_items"));
}

#[test]
fn should_preserve_cow_snapshots_across_savepoint_rollback() {
    // Arrange
    let session = CassieSession::new("postgres".to_string(), Some("cassie".to_string()));
    session.begin_transaction(None).expect("begin transaction");
    session
        .stage_document_write(
            "snapshot_savepoint_items",
            "item-a".to_string(),
            json!({"value": "kept"}),
        )
        .expect("stage row before savepoint");
    session
        .create_savepoint("before_more")
        .expect("create savepoint");
    let before_more = session.staged_write_snapshot("snapshot_savepoint_items");
    session
        .stage_document_write(
            "snapshot_savepoint_items",
            "item-b".to_string(),
            json!({"value": "rolled back"}),
        )
        .expect("stage row after savepoint");
    let after_more = session.staged_write_snapshot("snapshot_savepoint_items");

    // Act
    session
        .rollback_to_savepoint("before_more")
        .expect("rollback to savepoint");
    let restored = session.staged_write_snapshot("snapshot_savepoint_items");

    // Assert
    assert!(before_more.ordered_changes().contains_key("item-a"));
    assert!(!before_more.ordered_changes().contains_key("item-b"));
    assert!(after_more.ordered_changes().contains_key("item-b"));
    assert_eq!(restored.ordered_changes(), before_more.ordered_changes());
}

#[test]
fn should_report_no_changes_for_an_unmatched_collection_snapshot() {
    // Arrange
    let session = CassieSession::new("postgres".to_string(), Some("cassie".to_string()));
    session.begin_transaction(None).expect("begin transaction");
    session
        .stage_document_write(
            "cassie.public.snapshot_items",
            "item-a".to_string(),
            json!({"value": "present"}),
        )
        .expect("stage row");

    // Act
    let snapshot = session.staged_write_snapshot("other_items");

    // Assert
    assert!(snapshot.is_empty());
    assert!(!session.has_collection_changes("other_items"));
}
