use cassie::app::Cassie;
use cassie::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType};
use cassie::types::{Value, Vector};
use uuid::Uuid;

type MetricsValue = serde_json::Value;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-metrics-capacity-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn seed_capacity_fixture(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE metrics_capacity_docs (title TEXT, body TEXT, embedding VECTOR(3))",
            vec![],
        )
        .unwrap();
    let collection = cassie
        .catalog
        .get_schema("metrics_capacity_docs")
        .expect("catalog collection")
        .collection;
    cassie
        .execute_sql(
            session,
            "INSERT INTO metrics_capacity_docs (title, body, embedding) VALUES ('alpha', 'one two', $1)",
            vec![Value::Vector(Vector::new(vec![3.0, 4.0, 0.0]))],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE INDEX metrics_capacity_title_idx ON metrics_capacity_docs USING btree (title)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE INDEX metrics_capacity_body_idx ON metrics_capacity_docs USING fulltext (body)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE INDEX metrics_capacity_column_idx ON metrics_capacity_docs USING column (title, body) WITH (segment_size = 1)",
            vec![],
        )
        .unwrap();
    cassie
        .midge
        .put_vector_index(VectorIndexRecord {
            collection,
            field: "embedding".to_string(),
            source_field: "body".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::Cosine,
                index_type: VectorIndexType::BruteForce,
                hnsw: None,
                hnsw_graph: None,
                ivfflat: None,
                ivfflat_training: None,
            },
        })
        .unwrap();
}

fn assert_capacity_metadata(capacity: &MetricsValue) {
    assert_eq!(capacity["advisory"], true);
    assert_eq!(capacity["local_only"], true);
    assert_eq!(capacity["persisted_metadata"], false);
    assert!(capacity["total_bytes"].as_u64().unwrap() > 0);
}

fn assert_capacity_families(capacity: &MetricsValue) {
    assert_eq!(capacity["families"]["schema"]["supported"], true);
    assert_eq!(capacity["families"]["data"]["supported"], true);
    assert_eq!(capacity["families"]["temp"]["supported"], true);
    assert_eq!(capacity["families"]["default"]["supported"], true);
    assert!(
        capacity["families"]["schema"]["total_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        capacity["families"]["data"]["total_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
}

fn assert_capacity_categories(capacity: &MetricsValue) {
    for category in [
        "row_blobs",
        "scalar_indexes",
        "fulltext",
        "vector_sidecars",
        "column_batches",
        "projection_metadata",
        "temp_artifacts",
        "other",
    ] {
        assert_eq!(capacity["categories"][category]["supported"], true);
        assert!(capacity["categories"][category]["total_bytes"]
            .as_u64()
            .is_some());
    }

    for category in [
        "row_blobs",
        "scalar_indexes",
        "fulltext",
        "vector_sidecars",
        "column_batches",
    ] {
        assert!(
            capacity["categories"][category]["total_bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
}

#[test]
fn should_report_local_capacity_bytes_by_category() {
    // Arrange
    with_fallback();
    let path = data_dir("category-bytes");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_capacity_fixture(&cassie, &session);

        // Act
        let metrics = cassie.metrics();
        let capacity = &metrics["capacity"];

        // Assert
        assert_capacity_metadata(capacity);
        assert_capacity_families(capacity);
        assert_capacity_categories(capacity);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}
