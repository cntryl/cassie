//! Retired post-commit work must not mutate a replacement collection.

use super::{Cassie, CommittedCollection, CommittedWriteBatch};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

#[test]
fn should_skip_retired_batch_maintenance_after_a_same_generation_replacement() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE maintenance_recreated (payload TEXT)",
        "INSERT INTO maintenance_recreated VALUES ('old')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("old target");
    }
    let target = crate::catalog::canonical_relation_name(
        &cassie.default_database,
        crate::catalog::DEFAULT_SCHEMA,
        "maintenance_recreated",
    );
    let old_id = cassie.midge.row_schema(&target).expect("old descriptor").relation_id;
    let old_generation = cassie.midge.collection_generation(&target).expect("old generation");
    let delayed = CommittedWriteBatch {
        changed_collections: vec![CommittedCollection {
            collection: target.clone(),
            relation_id: old_id,
            row_delta: i64::MAX,
            changed_ids: vec!["retired-row".into()],
        }],
    };
    for sql in [
        "BEGIN",
        "DROP TABLE maintenance_recreated",
        "CREATE TABLE maintenance_recreated (payload TEXT)",
        "INSERT INTO maintenance_recreated VALUES ('new')",
        "COMMIT",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("replacement");
    }
    let new_id = cassie.midge.row_schema(&target).expect("new descriptor").relation_id;
    let projection_before = serde_json::to_value(
        cassie.midge.projection_metadata(&target).expect("projection metadata"),
    )
    .expect("projection value");
    let hashes_before = serde_json::to_value(
        cassie.midge.projection_hash_summary(&target).expect("hash metadata"),
    )
    .expect("hash value");
    let debt_before = serde_json::to_value(
        cassie.midge.list_maintenance_debt().expect("maintenance debt"),
    )
    .expect("debt value");

    // Act
    cassie.finish_staged_write_batches(delayed, None);

    // Assert
    assert!(old_id > 0 && new_id > 0);
    assert_ne!(old_id, new_id);
    assert_eq!(
        cassie.midge.collection_generation(&target).expect("new generation"),
        old_generation,
        "generation equality cannot authorize retired work"
    );
    assert_eq!(
        serde_json::to_value(cassie.midge.projection_metadata(&target).expect("projection"))
            .expect("value"),
        projection_before
    );
    assert_eq!(
        serde_json::to_value(cassie.midge.projection_hash_summary(&target).expect("hashes"))
            .expect("value"),
        hashes_before
    );
    assert_eq!(
        serde_json::to_value(cassie.midge.list_maintenance_debt().expect("debt"))
            .expect("value"),
        debt_before
    );
    let rows = cassie
        .execute_sql(&session, "SELECT payload FROM maintenance_recreated", vec![])
        .expect("replacement rows");
    assert_eq!(rows.rows(), &[vec![Value::String("new".into())]]);
}
