#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::types::{DataType, FieldSchema, Schema};

#[path = "support/sql.rs"]
mod support;
use support::*;

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
        let collection = "metrics_ordered_read_paths";
        let schema = Schema {
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
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(collection, schema);

        for (id, title, score) in [("d1", "one", 1), ("d2", "two", 2), ("d3", "three", 3)] {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(id.to_string()),
                    serde_json::json!({"title": title, "score": score}),
                )
                .unwrap();
        }

        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "SELECT id FROM metrics_ordered_read_paths ORDER BY id ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT id FROM metrics_ordered_read_paths WHERE id > 'd1' ORDER BY id ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT id FROM metrics_ordered_read_paths ORDER BY id ASC LIMIT 1 OFFSET 1",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT id FROM metrics_ordered_read_paths ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let after = cassie.metrics();
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
        let collection = "metrics_scalar_read_paths";
        let schema = Schema {
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
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(collection, schema);
        cassie
            .execute_sql(
                &cassie.create_session("bootstrap", None),
                "CREATE INDEX metrics_scalar_title_idx ON metrics_scalar_read_paths USING btree (title)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &cassie.create_session("bootstrap", None),
                "CREATE INDEX metrics_scalar_tenant_status_idx ON metrics_scalar_read_paths USING btree (tenant_id, status)",
                vec![],
            )
            .unwrap();

        for (tenant_id, status, title) in [
            ("tenant-a", "closed", "alpha"),
            ("tenant-a", "open", "beta"),
            ("tenant-b", "closed", "charlie"),
            ("tenant-b", "open", "delta"),
        ] {
            cassie
                .midge
                .put_document(
                    collection,
                    None,
                    serde_json::json!({
                        "tenant_id": tenant_id,
                        "status": status,
                        "title": title,
                    }),
                )
                .unwrap();
        }

        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_scalar_read_paths WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_scalar_read_paths WHERE tenant_id = 'tenant-a' AND status = 'open'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_scalar_read_paths WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_scalar_read_paths ORDER BY title ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        let after = cassie.metrics();
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

        let _ = std::fs::remove_dir_all(path);
    });
}
