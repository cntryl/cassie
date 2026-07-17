use cassie::app::Cassie;
use cassie::embeddings::{
    DistanceMetric, IvfFlatIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn seed_ivfflat(cassie: &Cassie, collection: &str) {
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "content".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(
        collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    for (id, embedding) in [
        ("near", [1.0, 0.0, 0.0]),
        ("middle", [0.5, 0.5, 0.0]),
        ("far", [-1.0, 0.0, 0.0]),
    ] {
        cassie
            .midge
            .put_document(
                collection,
                Some(id.to_string()),
                serde_json::json!({"content": id, "embedding": embedding}),
            )
            .expect("put vector document");
    }
    cassie
        .midge
        .put_vector_index(VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "content".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::L2,
                index_type: VectorIndexType::IvfFlat,
                hnsw: None,
                hnsw_graph: None,
                ivfflat: Some(IvfFlatIndexOptions {
                    version: 1,
                    lists: 2,
                    probes: 2,
                    training_sample_size: 3,
                    training_seed: 17,
                }),
                ivfflat_training: None,
            },
        })
        .expect("put IVFFlat index");
}

fn remove_one_membership(cassie: &Cassie, collection: &str) {
    let prefix = cassie
        .midge
        .ivfflat_membership_prefix_for_diagnostics(collection, "embedding")
        .expect("membership prefix");
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("membership scan")
        .into_iter()
        .next()
        .expect("persisted membership")
        .0;
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("data transaction");
    tx.delete(key).expect("delete membership");
    tx.commit(WriteOptions::sync())
        .expect("commit membership corruption");
}

fn execute_top_k(cassie: &Cassie, collection: &str) -> cassie::executor::QueryResult {
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            &format!(
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"
            ),
            vec![],
        )
        .expect("execute exact fallback query")
}

fn assert_exact_fallback(cassie: &Cassie, collection: &str, expected_reason: &str) {
    let before = cassie.metrics();
    let result = execute_top_k(cassie, collection);
    let after = cassie.metrics();

    assert_eq!(result.rows[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
            - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        after["vector"]["last_fallback_reason"].as_str(),
        Some(expected_reason)
    );
}

#[test]
fn should_fallback_exactly_given_missing_membership_in_probed_ivfflat_list() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_missing_probed_membership");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let collection = "ivfflat_missing_probed_membership";
    seed_ivfflat(&cassie, collection);
    remove_one_membership(&cassie, collection);

    // Act
    assert_exact_fallback(&cassie, collection, "stale-list-membership");

    // Assert
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_exactly_given_corrupt_ivfflat_list_count() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_corrupt_list_count");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let collection = "ivfflat_corrupt_list_count";
    seed_ivfflat(&cassie, collection);
    let mut state = cassie
        .midge
        .get_vector_index_state(collection, "embedding")
        .expect("read vector state")
        .expect("persisted vector state");
    state
        .ivfflat_training
        .as_mut()
        .expect("IVFFlat training")
        .list_sizes[0] += 1;
    cassie
        .midge
        .put_vector_index_state(collection, "embedding", state)
        .expect("persist corrupt list count");

    // Act
    assert_exact_fallback(&cassie, collection, "stale-list-sizes");

    // Assert
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_incomplete_ivfflat_membership_on_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_restart_rebuild");
    let collection = "ivfflat_restart_rebuild";
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        seed_ivfflat(&cassie, collection);
        remove_one_membership(&cassie, collection);
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");

    // Act
    restarted.startup().expect("repair startup");
    let before = restarted.metrics();
    let result = execute_top_k(&restarted, collection);
    let after = restarted.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
            - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
        0
    );
    assert_eq!(
        after["vector"]["ivfflat_executions"].as_u64().unwrap()
            - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
        1
    );
    let prefix = restarted
        .midge
        .ivfflat_membership_prefix_for_diagnostics(collection, "embedding")
        .expect("membership prefix");
    let observed_memberships = restarted
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("membership scan")
        .len();
    let (_, expected_memberships) = restarted
        .midge
        .get_ivfflat_training_manifest(collection, "embedding")
        .expect("read manifest")
        .expect("persisted manifest");
    assert_eq!(observed_memberships, expected_memberships);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_unsupported_ivfflat_training_version_on_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_version_rebuild");
    let collection = "ivfflat_version_rebuild";
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        seed_ivfflat(&cassie, collection);
        let mut state = cassie
            .midge
            .get_vector_index_state(collection, "embedding")
            .expect("read vector state")
            .expect("persisted vector state");
        state
            .ivfflat_training
            .as_mut()
            .expect("IVFFlat training")
            .version = 0;
        cassie
            .midge
            .put_vector_index_state(collection, "embedding", state)
            .expect("persist old training version");
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");

    // Act
    restarted.startup().expect("repair startup");
    let (training, _) = restarted
        .midge
        .get_ivfflat_training_manifest(collection, "embedding")
        .expect("read repaired manifest")
        .expect("repaired manifest");

    // Assert
    assert_eq!(training.version, 1);
    let _ = std::fs::remove_dir_all(path);
}
