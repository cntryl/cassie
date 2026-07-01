#![allow(unused_imports, dead_code)]

use cassie::app::{Cassie, CassieSession};
use cassie::types::{DataType, FieldSchema, Schema};
use serde_json::Value as JsonValue;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn ordered_read_schema() -> Schema {
    Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    }
}

fn scalar_read_schema() -> Schema {
    Schema {
        fields: vec![
            FieldSchema {
                name: "tenant_id".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    }
}

fn register_collection_with_schema(cassie: &Cassie, collection: &str, schema: Schema) {
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(collection, schema);
}

fn insert_documents(cassie: &Cassie, collection: &str, docs: Vec<(Option<String>, JsonValue)>) {
    for (id, payload) in docs {
        cassie.midge.put_document(collection, id, payload).unwrap();
    }
}

fn seed_ordered_read_collection(cassie: &Cassie, collection: &str) {
    register_collection_with_schema(cassie, collection, ordered_read_schema());
    insert_documents(
        cassie,
        collection,
        vec![
            (
                Some("d1".to_string()),
                serde_json::json!({"title": "one", "score": 1}),
            ),
            (
                Some("d2".to_string()),
                serde_json::json!({"title": "two", "score": 2}),
            ),
            (
                Some("d3".to_string()),
                serde_json::json!({"title": "three", "score": 3}),
            ),
        ],
    );
}

fn seed_scalar_read_collection(cassie: &Cassie, collection: &str) {
    register_collection_with_schema(cassie, collection, scalar_read_schema());
    let bootstrap = cassie.create_session("bootstrap", None);
    for sql in [
        "CREATE INDEX metrics_scalar_title_idx ON metrics_scalar_read_paths USING btree (title)",
        "CREATE INDEX metrics_scalar_tenant_status_idx ON metrics_scalar_read_paths USING btree (tenant_id, status)",
    ] {
        cassie.execute_sql(&bootstrap, sql, vec![]).unwrap();
    }
    insert_documents(
        cassie,
        collection,
        vec![
            (
                None,
                serde_json::json!({
                    "tenant_id": "tenant-a",
                    "status": "closed",
                    "title": "alpha",
                }),
            ),
            (
                None,
                serde_json::json!({
                    "tenant_id": "tenant-a",
                    "status": "open",
                    "title": "beta",
                }),
            ),
            (
                None,
                serde_json::json!({
                    "tenant_id": "tenant-b",
                    "status": "closed",
                    "title": "charlie",
                }),
            ),
            (
                None,
                serde_json::json!({
                    "tenant_id": "tenant-b",
                    "status": "open",
                    "title": "delta",
                }),
            ),
        ],
    );
}

fn execute_queries(cassie: &Cassie, session: &CassieSession, queries: &[&str]) {
    for sql in queries {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }
}

fn assert_ordered_read_metrics(before: &serde_json::Value, after: &serde_json::Value) {
    assert_eq!(
        after["read_paths"]["ordered_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["ordered_scans"]
            .as_u64()
            .unwrap_or_default()
            + 4,
    );
    assert_eq!(
        after["read_paths"]["ordered_rows"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["ordered_rows"]
            .as_u64()
            .unwrap_or_default()
            + 5,
    );
    assert_eq!(
        after["read_paths"]["storage_top_k_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["storage_top_k_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["keyset_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["keyset_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["degraded_offset_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["degraded_offset_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["heap_top_k_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["heap_top_k_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["last_ordered_scan_mode"].as_str(),
        Some("heap_top_k"),
    );
}

fn assert_scalar_read_metrics(before: &serde_json::Value, after: &serde_json::Value) {
    assert_eq!(
        after["read_paths"]["index_seek_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["index_seek_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["prefix_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["prefix_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["range_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["range_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["ordered_bounded_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["read_paths"]["ordered_bounded_scans"]
            .as_u64()
            .unwrap_or_default()
            + 1,
    );
    assert_eq!(
        after["read_paths"]["last_index_scan_mode"].as_str(),
        Some("ordered_bounded_scan"),
    );
    assert_eq!(
        after["read_paths"]["last_index_scan_index"].as_str(),
        Some("metrics_scalar_title_idx"),
    );
}

#[test]
fn should_record_runtime_metrics_for_ordered_read_paths() {
    // Arrange
    with_fallback();
    let path = data_dir("metrics_read_paths_ordered");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        seed_ordered_read_collection(&cassie, "metrics_ordered_read_paths");
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        execute_queries(
            &cassie,
            &session,
            &[
                "SELECT id FROM metrics_ordered_read_paths ORDER BY id ASC LIMIT 2",
                "SELECT id FROM metrics_ordered_read_paths WHERE id > 'd1' ORDER BY id ASC LIMIT 1",
                "SELECT id FROM metrics_ordered_read_paths ORDER BY id ASC LIMIT 1 OFFSET 1",
                "SELECT id FROM metrics_ordered_read_paths ORDER BY score DESC LIMIT 1",
            ],
        );

        // Assert
        let after = cassie.metrics();
        assert_ordered_read_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_runtime_metrics_for_scalar_index_read_paths() {
    // Arrange
    with_fallback();
    let path = data_dir("metrics_read_paths_scalar");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        seed_scalar_read_collection(&cassie, "metrics_scalar_read_paths");
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        execute_queries(
            &cassie,
            &session,
            &[
                "SELECT title FROM metrics_scalar_read_paths WHERE title = 'alpha'",
                "SELECT title FROM metrics_scalar_read_paths WHERE tenant_id = 'tenant-a' AND status = 'open'",
                "SELECT title FROM metrics_scalar_read_paths WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                "SELECT title FROM metrics_scalar_read_paths ORDER BY title ASC LIMIT 2",
            ],
        );

        // Assert
        let after = cassie.metrics();
        assert_scalar_read_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
}
