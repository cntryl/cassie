use std::mem::size_of;
use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::embeddings::{
    DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use crate::midge::adapter::query_scan_control_test_guard;
use crate::types::{DataType, FieldSchema, Schema};
use crate::vector::hnsw::HnswCandidate;

use super::{Midge, QueryExecutionControls};

#[test]
fn should_reject_hnsw_node_decode_memory_before_a_later_corrupt_neighbor() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let collection = "controlled_hnsw_decode";
    let (midge, path, _) =
        controlled_hnsw_fixture("hnsw-node-decode-budget", collection, ["entry".to_owned()]);
    let canary = crate::embeddings::HnswGraphNode {
        id: "entry".to_owned(),
        vector: vec![1.0, 0.0, 0.0],
        magnitude: 1.0,
        layers: vec![Vec::new(), vec!["x".to_owned()]],
    };
    let mut raw = super::super::codec::encode_hnsw_node(&canary).expect("encode canary");
    *raw.last_mut().expect("neighbor byte") = u8::MAX;
    let (relation_id, field_id) = midge
        .vector_storage_ids(collection, "embedding")
        .expect("storage IDs");
    let key = super::key_encoding::hnsw_graph_node_key(relation_id, field_id, "entry");
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &raw)
        .expect("persist late decode canary");
    let index = midge
        .get_vector_index_definition(collection, "embedding")
        .expect("stored index definition")
        .expect("HNSW index");
    let state_key = midge
        .vector_state_key_for_diagnostics(collection, "embedding")
        .expect("state key");
    let state_raw = midge
        .raw_get(crate::midge::adapter::StorageFamily::Data, &state_key)
        .expect("read state")
        .expect("persisted state");
    let summary_key = super::key_encoding::hnsw_source_summary_key(relation_id, field_id);
    let summary_raw = midge
        .raw_get(crate::midge::adapter::StorageFamily::Data, &summary_key)
        .expect("read summary")
        .expect("persisted summary");
    // Permit valid metadata and query preparation, retaining 240 bytes for node decode.
    let metadata_bytes = super::accounting::vector_state_bytes(state_raw.len())
        .expect("state bound")
        + super::accounting::source_summary_bytes(summary_raw.len()).expect("summary bound");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: metadata_bytes
                + 2 * 3 * size_of::<f32>()
                + "entry".len()
                + 240,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let before_reads = midge.query_scan_entries_for_diagnostics();

    // Act
    let result = midge.search_hnsw_graph_point_read_controlled(
        collection,
        "embedding",
        &[1.0, 0.0, 0.0],
        &index.metadata,
        1,
        &controls,
    );
    let error = result.err().expect("node decode must reject the query");
    let reads = midge.query_scan_entries_for_diagnostics() - before_reads;
    let remaining_bytes = controls.current_query_memory_bytes();
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");

    // Assert
    assert!(
        matches!(error, crate::app::CassieError::ResourceLimit(_)),
        "the decoded-node reservation must win before the corrupt neighbor canary: {error:?}"
    );
    assert_eq!(
        reads, 3,
        "metadata succeeds and the first node decode rejects"
    );
    assert_eq!(remaining_bytes, 0);
}

#[test]
fn should_reserve_the_retained_hnsw_candidate_capacity_after_truncating_top_k() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let collection = "controlled_hnsw_capacity";
    let (midge, path, options) = controlled_hnsw_fixture(
        "hnsw-candidate-capacity",
        collection,
        (0..40).map(|index| format!("row-{index:02}")),
    );
    let mut state = midge
        .get_vector_index_state(collection, "embedding")
        .expect("stored state")
        .expect("HNSW state");
    let graph = state.hnsw_graph.as_mut().expect("graph");
    let ids = graph
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    // A complete base layer makes the ef_search-sized candidate set deterministic.
    graph.max_layer = 0;
    graph.entry_point = ids.first().cloned();
    for node in &mut graph.nodes {
        node.layers = vec![ids.iter().filter(|id| **id != node.id).cloned().collect()];
    }
    midge
        .put_vector_index_state(collection, "embedding", state)
        .expect("persist complete base layer");
    let index = midge
        .get_vector_index_definition(collection, "embedding")
        .expect("stored index definition")
        .expect("HNSW index");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

    // Act
    let batch = midge
        .search_hnsw_graph_point_read_controlled(
            collection,
            "embedding",
            &[1.0, 0.0, 0.0],
            &index.metadata,
            1,
            &controls,
        )
        .expect("controlled HNSW search")
        .expect("candidate batch");
    let candidate_count = batch.candidate_count;
    let returned_count = batch.candidates.len();
    let retained_capacity = batch.candidates.capacity();
    let retained_bytes = retained_capacity * size_of::<HnswCandidate>()
        + batch
            .candidates
            .iter()
            .map(|candidate| candidate.id.len())
            .sum::<usize>();
    let reserved_bytes = batch.candidate_memory.bytes();
    let live_bytes = controls.current_query_memory_bytes();
    drop(batch);
    let remaining_bytes = controls.current_query_memory_bytes();
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");

    // Assert
    assert_eq!(candidate_count, options.ef_search);
    assert_eq!(returned_count, 1);
    assert!(
        reserved_bytes >= retained_bytes,
        "candidate Vec capacity {retained_capacity} retains {retained_bytes} bytes, but its guard reserves {reserved_bytes}"
    );
    assert_eq!(live_bytes, reserved_bytes);
    assert_eq!(remaining_bytes, 0);
}

#[test]
fn should_reject_ivfflat_decode_overlap_before_a_later_corrupt_candidate() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let path = std::env::temp_dir().join(format!(
        "cassie-ivfflat-candidate-decode-budget-{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
    midge.ensure_families_ready().expect("storage families");
    let collection = "controlled_ivf_decode";
    let field = "embedding";
    let dimensions = 8193;
    midge
        .create_collection(
            collection,
            Schema {
                fields: vec![FieldSchema {
                    name: field.to_owned(),
                    data_type: DataType::Vector(dimensions),
                    nullable: false,
                }],
            },
        )
        .expect("vector collection");
    let record = crate::embeddings::NormalizedVectorRecord {
        built_generation: 0,
        collection: collection.to_owned(),
        field: field.to_owned(),
        id: "wide".to_owned(),
        dimensions,
        metric: DistanceMetric::L2,
        normalization_version:
            crate::embeddings::NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
        payload_available: true,
        magnitude: 1.0,
        values: vec![1.0; dimensions],
    };
    let raw = super::super::codec::encode_normalized_vector(&record)
        .expect("encode valid wide candidate");
    let minimum_live_bytes = size_of::<crate::embeddings::NormalizedVectorRecord>()
        + raw.len()
        + dimensions * size_of::<f32>()
        + collection.len()
        + field.len()
        + record.id.len();
    let (relation_id, field_id) = midge
        .vector_storage_ids(collection, field)
        .expect("storage IDs");
    let key = super::key_encoding::normalized_vector_key(relation_id, field_id, &record.id);
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &raw)
        .expect("persist valid wide candidate");
    let mut canary = raw[..27].to_vec();
    canary[17] = u8::MAX;
    let canary_key = super::key_encoding::normalized_vector_key(relation_id, field_id, "canary");
    midge
        .raw_put(
            crate::midge::adapter::StorageFamily::Data,
            &canary_key,
            &canary,
        )
        .expect("persist corrupt later candidate");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: minimum_live_bytes - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let tx = midge
        .begin_data_readonly_tx_for(collection)
        .expect("candidate read transaction");
    let context = super::ControlledIvfReadContext {
        midge: &midge,
        tx: &tx,
        collection,
        field,
        relation_id,
        field_id,
        controls: &controls,
    };

    // Act
    let result = super::load_controlled_ivfflat_vectors(
        &context,
        vec![record.id.clone(), "canary".to_owned()],
        0,
    );
    let error = result.expect_err("candidate decode must reject the query");
    let remaining_bytes = controls.current_query_memory_bytes();
    drop(tx);
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");

    // Assert
    assert!(
        matches!(error, crate::app::CassieError::ResourceLimit(_)),
        "the {minimum_live_bytes}-byte raw/decoded overlap must reject before the corrupt later candidate: {error:?}"
    );
    assert_eq!(remaining_bytes, 0);
}

fn controlled_hnsw_fixture(
    name: &str,
    collection: &str,
    ids: impl IntoIterator<Item = String>,
) -> (Midge, std::path::PathBuf, HnswIndexOptions) {
    let path = std::env::temp_dir().join(format!("cassie-{name}-{}", uuid::Uuid::new_v4()));
    let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
    midge.ensure_families_ready().expect("storage families");
    midge
        .create_collection(
            collection,
            Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_owned(),
                    data_type: DataType::Vector(3),
                    nullable: false,
                }],
            },
        )
        .expect("vector collection");
    for id in ids {
        midge
            .put_document(
                collection,
                Some(id),
                serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
            )
            .expect("vector row");
    }
    let options = HnswIndexOptions::default();
    midge
        .put_vector_index(VectorIndexRecord {
            collection: collection.to_owned(),
            field: "embedding".to_owned(),
            source_field: "embedding".to_owned(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_owned(),
                model: "manual".to_owned(),
                dimensions: 3,
                metric: DistanceMetric::L2,
                index_type: VectorIndexType::Hnsw,
                hnsw: Some(options.clone()),
                hnsw_graph: None,
                ivfflat: None,
                ivfflat_training: None,
            },
        })
        .expect("HNSW index");
    (midge, path, options)
}

fn controlled_ivf_fixture(name: &str, dimensions: usize) -> (Midge, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("cassie-{name}-{}", uuid::Uuid::new_v4()));
    let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
    midge.ensure_families_ready().expect("storage families");
    midge
        .create_collection(
            "controlled_ivf",
            Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_owned(),
                    data_type: DataType::Vector(dimensions),
                    nullable: false,
                }],
            },
        )
        .expect("vector collection");
    (midge, path)
}

#[test]
fn should_keep_decoded_ivfflat_manifest_capacities_reserved_until_drop() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let dimensions = 8193;
    let (midge, path) = controlled_ivf_fixture("ivfflat-manifest-capacity", dimensions);
    midge
        .put_document(
            "controlled_ivf",
            Some("row".to_owned()),
            serde_json::json!({"embedding": vec![1.0; dimensions]}),
        )
        .expect("source vector row");
    let generation = midge
        .collection_generation("controlled_ivf")
        .expect("source generation");
    let state = super::super::codec::PersistedVectorIndexState {
        built_generation: generation,
        hnsw_graph: None,
        ivfflat_training: Some(super::super::codec::PersistedIvfManifest {
            version: crate::vector::ivfflat::TRAINING_VERSION,
            source_fingerprint: 1,
            trained: true,
            row_count: 1,
            lists: 1,
            probes: 1,
            training_seed: 17,
            centroid_ids: vec!["row".to_owned()],
            centroids: vec![vec![1.0; dimensions]],
            list_sizes: vec![1],
            membership_count: 1,
        }),
    };
    let raw = super::super::codec::encode_vector_index_state(&state)
        .expect("encode supported IVF manifest");
    let key = midge
        .vector_state_key_for_diagnostics("controlled_ivf", "embedding")
        .expect("state key");
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &raw)
        .expect("persist IVF manifest");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

    // Act
    let snapshot = midge
        .get_ivfflat_training_manifest_controlled("controlled_ivf", "embedding", &controls)
        .expect("controlled manifest read")
        .expect("IVF manifest");
    let training = &snapshot.training;
    let retained_bytes = training.centroid_ids.capacity() * size_of::<String>()
        + training
            .centroid_ids
            .iter()
            .map(String::capacity)
            .sum::<usize>()
        + training.centroids.capacity() * size_of::<Vec<f32>>()
        + training
            .centroids
            .iter()
            .map(|centroid| centroid.capacity() * size_of::<f32>())
            .sum::<usize>()
        + training.list_sizes.capacity() * size_of::<usize>();
    let reserved_bytes = snapshot.manifest_memory.bytes();
    let live_bytes = controls.current_query_memory_bytes();
    drop(snapshot);
    let remaining_bytes = controls.current_query_memory_bytes();
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");

    // Assert
    assert!(
        reserved_bytes >= retained_bytes,
        "decoded IVF manifest retains at least {retained_bytes} heap bytes, but its guard reserves {reserved_bytes}"
    );
    assert_eq!(live_bytes, reserved_bytes);
    assert_eq!(remaining_bytes, 0);
}

#[test]
fn should_reserve_complete_sparse_ivfflat_membership_state() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let (midge, path) = controlled_ivf_fixture("ivfflat-membership-budget", 3);
    let (relation_id, field_id) = midge
        .vector_storage_ids("controlled_ivf", "embedding")
        .expect("storage IDs");
    let id = "a";
    let key = super::key_encoding::ivfflat_membership_key(relation_id, field_id, 0, id);
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &[])
        .expect("persist valid membership");
    // A sparse BTree leaf holds eleven String slots, in addition to the ID Vec and both copies.
    let minimum_live_bytes = 11 * size_of::<String>() + size_of::<String>() + 2 * id.len();
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: minimum_live_bytes - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let tx = midge
        .begin_data_readonly_tx_for("controlled_ivf")
        .expect("membership read transaction");
    let context = super::ControlledIvfReadContext {
        midge: &midge,
        tx: &tx,
        collection: "controlled_ivf",
        field: "embedding",
        relation_id,
        field_id,
        controls: &controls,
    };
    let training = crate::embeddings::IvfFlatTrainingState {
        version: crate::vector::ivfflat::TRAINING_VERSION,
        source_fingerprint: 1,
        trained: true,
        row_count: 1,
        lists: 1,
        probes: 1,
        training_seed: 17,
        centroid_ids: vec![id.to_owned()],
        centroids: vec![vec![1.0, 0.0, 0.0]],
        assignments: std::collections::BTreeMap::new(),
        list_sizes: vec![1],
    };
    let probed_lists = std::collections::BTreeSet::from([0]);

    // Act
    let result = super::load_controlled_ivfflat_memberships(&context, &training, &probed_lists);
    let rejected = matches!(result, Err(crate::app::CassieError::ResourceLimit(_)));
    drop(result);
    let remaining_bytes = controls.current_query_memory_bytes();
    drop(tx);
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");

    // Assert
    assert!(
        rejected,
        "the valid membership's {minimum_live_bytes}-byte sparse-node and identity lower bound exceeds the budget"
    );
    assert_eq!(remaining_bytes, 0);
}

#[test]
fn should_reject_hnsw_state_entry_copy_before_a_later_corrupt_summary() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let (midge, path) = controlled_ivf_fixture("hnsw-header-decode-budget", 3);
    let entry_point = "x".repeat(8192);
    midge
        .put_document(
            "controlled_ivf",
            Some(entry_point.clone()),
            serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
        )
        .expect("source vector row");
    let generation = midge
        .collection_generation("controlled_ivf")
        .expect("source generation");
    let state = super::super::codec::PersistedVectorIndexState {
        built_generation: generation,
        hnsw_graph: Some(super::super::codec::PersistedHnswManifest {
            version: crate::vector::hnsw::HNSW_GRAPH_VERSION,
            source_fingerprint: 1,
            row_count: 1,
            dimensions: 3,
            metric: DistanceMetric::L2,
            entry_point: Some(entry_point.clone()),
            max_layer: 0,
        }),
        ivfflat_training: None,
    };
    let raw = super::super::codec::encode_vector_index_state(&state)
        .expect("encode supported HNSW header");
    let minimum_live_bytes = raw.len() + entry_point.len();
    let (relation_id, field_id) = midge
        .vector_storage_ids("controlled_ivf", "embedding")
        .expect("storage IDs");
    let state_key = midge
        .vector_state_key_for_diagnostics("controlled_ivf", "embedding")
        .expect("state key");
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &state_key, &raw)
        .expect("persist HNSW header");
    let summary_key = super::key_encoding::hnsw_source_summary_key(relation_id, field_id);
    midge
        .raw_put(
            crate::midge::adapter::StorageFamily::Data,
            &summary_key,
            b"{",
        )
        .expect("persist later corrupt summary canary");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: minimum_live_bytes - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let tx = midge
        .begin_data_readonly_tx_for("controlled_ivf")
        .expect("header read transaction");

    // Act
    let result = super::load_controlled_hnsw_header(
        &midge,
        &tx,
        "controlled_ivf",
        relation_id,
        field_id,
        &controls,
    );
    let error = result.err().expect("header decode must reject the query");
    let remaining_bytes = controls.current_query_memory_bytes();
    drop(tx);
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");

    // Assert
    assert!(
        matches!(error, crate::app::CassieError::ResourceLimit(_)),
        "the {minimum_live_bytes}-byte raw/header copy must reject before the corrupt summary: {error:?}"
    );
    assert_eq!(remaining_bytes, 0);
}
