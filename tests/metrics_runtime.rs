use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, IndexKind, IndexMeta};
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

#[path = "metrics_runtime/projections.rs"]
mod projections;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn canonical_public_relation(name: &str) -> String {
    canonical_relation_name("postgres", "public", name)
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-metrics-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

struct ReadPathBaseline {
    point_hits: u64,
    point_misses: u64,
    point_scans: u64,
    collection_scans: u64,
}

fn seed_read_path_collection(cassie: &Cassie) {
    let collection = "metrics_read_paths";
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_string(),
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
    for (id, payload) in [
        (
            "doc-1",
            serde_json::json!({"title": "alpha", "status": "active"}),
        ),
        (
            "doc-2",
            serde_json::json!({"title": "bravo", "status": "queued"}),
        ),
    ] {
        cassie
            .midge
            .put_document(collection, Some(id.to_string()), payload)
            .unwrap();
    }
}

fn seed_cardinality_metrics_fixture(cassie: &Cassie, collection: &str, index: &str) {
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };

    cassie.midge.create_database("postgres", None).unwrap();
    cassie.midge.create_namespace("postgres.public").unwrap();
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha", "body": "bravo"}),
        )
        .unwrap();
    cassie
        .midge
        .put_index(&IndexMeta {
            collection: collection.to_string(),
            name: index.to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::default(),
        })
        .unwrap();
    cassie.midge.delete_cardinality_stats(collection).unwrap();
}

fn read_path_baseline(metrics: &serde_json::Value) -> ReadPathBaseline {
    ReadPathBaseline {
        point_hits: metrics["read_paths"]["point_lookup_hits"]
            .as_u64()
            .unwrap_or_default(),
        point_misses: metrics["read_paths"]["point_lookup_misses"]
            .as_u64()
            .unwrap_or_default(),
        point_scans: metrics["read_paths"]["point_lookup_scans"]
            .as_u64()
            .unwrap_or_default(),
        collection_scans: metrics["read_paths"]["collection_scans"]
            .as_u64()
            .unwrap_or_default(),
    }
}

fn execute_read_path_queries(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for sql in [
        "SELECT title FROM metrics_read_paths WHERE id = 'doc-1'",
        "SELECT title FROM metrics_read_paths WHERE id = 'missing'",
        "SELECT title FROM metrics_read_paths",
    ] {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }
}

fn assert_read_path_metrics(after: &serde_json::Value, before: &ReadPathBaseline) {
    assert_eq!(
        after["read_paths"]["point_lookup_scans"]
            .as_u64()
            .unwrap_or_default(),
        before.point_scans + 2,
    );
    assert_eq!(
        after["read_paths"]["point_lookup_hits"]
            .as_u64()
            .unwrap_or_default(),
        before.point_hits + 1,
    );
    assert_eq!(
        after["read_paths"]["point_lookup_misses"]
            .as_u64()
            .unwrap_or_default(),
        before.point_misses + 1,
    );
    assert_eq!(
        after["read_paths"]["collection_scans"]
            .as_u64()
            .unwrap_or_default(),
        before.collection_scans + 1,
    );
    assert!(after["read_paths"]["last_point_lookup_collection"]
        .as_str()
        .is_some());
}

#[test]
fn should_report_runtime_metrics_snapshot() {
    // Arrange
    with_fallback();
    let path = data_dir("startup_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "metrics_runtime_docs";
        let canonical_collection = canonical_public_relation(collection);
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(&canonical_collection, schema.clone())
            .unwrap();
        cassie.register_collection(&canonical_collection, schema.clone());
        cassie
            .midge
            .put_document(
                &canonical_collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["ready"], serde_json::Value::Bool(true));
        assert!(
            metrics["runtime"]["startup_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "startup counter should be recorded"
        );
        assert!(
            metrics["runtime"]["catalog_hydration_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "catalog hydration counter should be recorded"
        );
        assert_eq!(metrics["query"]["count"].as_u64(), Some(2));
        assert_eq!(metrics["query"]["rows_returned_total"].as_u64(), Some(2));
        assert!(
            metrics["storage"]["schema"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "schema storage reads should be recorded"
        );
        assert!(
            metrics["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "data storage reads should be recorded"
        );
        assert!(
            metrics["storage"]["temp"]["writes"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "temp storage writes should be recorded"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_read_path_metrics_for_point_lookup_collection_scan() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        seed_read_path_collection(&cassie);
        let session = cassie.create_session("tester", None);
        let before = read_path_baseline(&cassie.metrics());

        // Act
        execute_read_path_queries(&cassie, &session);

        // Assert
        let after = cassie.metrics();
        assert_read_path_metrics(&after, &before);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_expose_cardinality_metrics_with_explain_plan_estimates() {
    // Arrange
    with_fallback();
    let path = data_dir("cardinality_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = canonical_public_relation("metrics_cardinality_docs");
        let index = canonical_public_relation("idx_title");
        seed_cardinality_metrics_fixture(&cassie, &collection, &index);

        // Act
        cassie.startup().unwrap();
        cassie
            .ingest_document(
                &collection,
                serde_json::json!({"title": "beta", "body": "charlie"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM metrics_cardinality_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();
        let metrics = cassie.metrics();

        // Assert
        assert!(plan.contains("estimates=scan:2"), "plan={plan}");
        assert!(plan.contains("index:1"), "plan={plan}");
        assert!(plan.contains("cost_source=advanced_stats"), "plan={plan}");
        assert!(
            metrics["cardinality"]["reads"].as_u64().unwrap_or_default() >= 1,
            "cardinality reads should be tracked"
        );
        assert!(
            metrics["cardinality"]["writes"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "cardinality writes should be tracked"
        );
        assert!(
            metrics["cardinality"]["rebuilds"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "cardinality rebuilds should be tracked"
        );
        assert!(
            metrics["cardinality"]["unavailable"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "missing stats should be tracked"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_query_error_statistics() {
    // Arrange
    with_fallback();
    let path = data_dir("query_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();
        let before_count = before["query"]["count"].as_u64().unwrap_or_default();
        let before_errors = before["query"]["errors_total"].as_u64().unwrap_or_default();

        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT title FROM metrics_missing_query_errors",
            vec![],
        );
        let after = cassie.metrics();

        // Assert
        assert!(result.is_err(), "missing collection should fail");
        assert_eq!(
            after["query"]["count"].as_u64().unwrap_or_default() - before_count,
            1
        );
        assert_eq!(
            after["query"]["errors_total"].as_u64().unwrap_or_default() - before_errors,
            1
        );
        assert!(after["query"]["errors_by_class"]
            .as_object()
            .expect("errors by class")
            .values()
            .any(|count| count.as_u64().unwrap_or_default() > 0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_count_failed_scan_as_storage_read_error() {
    // Arrange
    with_fallback();
    let path = data_dir("scan_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.catalog.register_collection(
            "missing_storage_collection",
            vec![("title".to_string(), DataType::Text)],
        );

        let before = cassie.metrics();
        let before_errors = before["storage"]["data"]["errors"]
            .as_u64()
            .unwrap_or_default();
        let before_reads = before["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        let session = cassie.create_session("tester", None);
        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT title FROM missing_storage_collection WHERE title = 'alpha'",
            vec![],
        );
        assert!(
            result.is_err(),
            "query should fail because collection schema is missing in storage"
        );

        let after = cassie.metrics();

        // Assert
        assert_eq!(
            after["storage"]["data"]["errors"]
                .as_u64()
                .unwrap_or_default()
                - before_errors,
            1
        );
        assert!(
            after["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > before_reads,
            "scan failure should still record the read attempt"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
