#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::CollectionCardinalityStats;

#[path = "support/sql.rs"]
mod support;

use support::{canonical_test_collection, data_dir, with_fallback};

fn metric_delta(after: &serde_json::Value, before: &serde_json::Value, key: &str) -> u64 {
    after["cardinality"][key]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(before["cardinality"][key].as_u64().unwrap_or_default())
}

#[test]
fn should_increment_cardinality_for_unindexed_document_write_without_rebuild() {
    // Arrange
    with_fallback();
    let path = data_dir("incremental_unindexed_cardinality");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_docs (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "incremental_docs");
        let before = cassie.metrics();

        // Act
        let id = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"title": "alpha", "body": "bravo"}),
            )
            .unwrap();
        let after_insert = cassie.metrics();
        let insert_stats = cassie
            .catalog
            .get_cardinality_stats(&collection)
            .expect("insert cardinality stats");
        cassie::rest::documents::delete(&cassie, &collection, &id).unwrap();
        let after_delete = cassie.metrics();

        // Assert
        assert_eq!(insert_stats.row_count, 1);
        assert!(insert_stats.hydrated);
        let delete_stats = cassie
            .catalog
            .get_cardinality_stats(&collection)
            .expect("incremental cardinality stats");
        assert_eq!(delete_stats.row_count, 0);
        assert!(delete_stats.hydrated);
        assert_eq!(metric_delta(&after_insert, &before, "rebuilds"), 0);
        assert_eq!(metric_delta(&after_insert, &before, "writes"), 1);
        assert_eq!(metric_delta(&after_delete, &before, "rebuilds"), 0);
        assert_eq!(metric_delta(&after_delete, &before, "writes"), 2);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rebuild_cardinality_when_index_membership_changes() {
    // Arrange
    with_fallback();
    let path = data_dir("indexed_cardinality_rebuild");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE indexed_docs (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "indexed_docs");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_indexed_title ON indexed_docs USING btree (title)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        cassie
            .ingest_document(
                &collection,
                serde_json::json!({"title": "alpha", "body": "bravo"}),
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        let stats = cassie
            .catalog
            .get_cardinality_stats(&collection)
            .expect("indexed cardinality stats");
        let index = cassie
            .catalog
            .get_index(&collection, "idx_indexed_title")
            .expect("index metadata");
        assert_eq!(stats.row_count, 1);
        assert_eq!(
            stats.index_cardinality(&CollectionCardinalityStats::scalar_index_key(&index.name)),
            Some(1)
        );
        assert_eq!(metric_delta(&after, &before, "rebuilds"), 1);
        assert_eq!(metric_delta(&after, &before, "writes"), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}
