use std::time::Instant;

use super::rank_ivfflat_candidate_ids;
use crate::config::CassieRuntimeLimits;
use crate::embeddings::{DistanceMetric, NormalizedVectorRecord};
use crate::runtime::QueryExecutionControls;

fn controls(memory_budget_bytes: usize) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: memory_budget_bytes,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

fn record(id: &str, metric: DistanceMetric, values: &[f32]) -> NormalizedVectorRecord {
    let normalized = crate::vector::normalize(values).expect("normalized record");
    NormalizedVectorRecord {
        collection: "items".to_string(),
        field: "embedding".to_string(),
        id: id.to_string(),
        built_generation: 1,
        dimensions: values.len(),
        metric,
        normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
        payload_available: true,
        magnitude: normalized.magnitude,
        values: normalized.values,
    }
}

fn ivfflat_hybrid_fixture(
    build_index: bool,
) -> (crate::app::Cassie, super::super::HybridTopKSpec) {
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.embeddings =
        crate::config::EmbeddingsRuntimeConfig::Local(crate::config::LocalRuntimeConfig {
            model: "hybrid-test".to_string(),
            dimensions: 2,
        });
    let cassie = crate::app::Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, "CREATE EXTENSION vector", Vec::new())
        .expect("install vector extension");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ivf_rank (id BIGINT PRIMARY KEY, body TEXT, embedding VECTOR(2))",
            Vec::new(),
        )
        .expect("create vector table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO ivf_rank (id, body, embedding) VALUES \
                 (1, 'match', '[10,0]'), (2, 'match', '[1,0]')",
            Vec::new(),
        )
        .expect("insert vector rows");
    if build_index {
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX ivf_rank_embedding_idx ON ivf_rank USING vector (embedding) \
                     WITH (source_field = body, metric = l2, index_type = ivfflat, \
                     lists = 1, probes = 1, training_sample_size = 2, training_seed = 17)",
                Vec::new(),
            )
            .expect("create trained IVFFlat index");
    }
    let collection = crate::catalog::canonical_relation_name(
        &cassie.default_database,
        crate::catalog::DEFAULT_SCHEMA,
        "ivf_rank",
    );
    let spec = super::super::HybridTopKSpec {
        collection,
        text_field: "body".to_string(),
        query: "match".to_string(),
        vector_field: "embedding".to_string(),
        vector_query: vec![1.0, 0.0],
        filter: None,
        id_column: "id".to_string(),
        score_column: "score".to_string(),
        limit: 1,
        offset: 0,
    };
    (cassie, spec)
}

#[test]
fn should_rank_ivfflat_hybrid_candidates_before_applying_the_limit() {
    // Arrange
    let records = vec![
        record("far-first", DistanceMetric::L2, &[10.0, 0.0]),
        record("near-late", DistanceMetric::L2, &[1.0, 0.0]),
    ];

    // Act
    let selected = rank_ivfflat_candidate_ids(records, &[1.0, 0.0], 1, &controls(4096))
        .expect("rank candidates")
        .0;

    // Assert
    assert_eq!(
        selected.iter().map(String::as_str).collect::<Vec<_>>(),
        ["near-late"]
    );
}

#[test]
fn should_keep_stable_ivfflat_ties_with_under_limit_candidates() {
    // Arrange
    let records = vec![
        record("z-tie", DistanceMetric::L2, &[1.0, 0.0]),
        record("a-tie", DistanceMetric::L2, &[1.0, 0.0]),
    ];

    // Act
    let tied = rank_ivfflat_candidate_ids(records.clone(), &[1.0, 0.0], 1, &controls(4096))
        .expect("rank tied candidates")
        .0;
    let all = rank_ivfflat_candidate_ids(records, &[1.0, 0.0], 3, &controls(4096))
        .expect("keep under-limit candidates")
        .0;

    // Assert
    assert_eq!(
        tied.iter().map(String::as_str).collect::<Vec<_>>(),
        ["a-tie"]
    );
    assert_eq!(
        all.iter().map(String::as_str).collect::<Vec<_>>(),
        ["a-tie", "z-tie"]
    );
}

#[test]
fn should_reject_an_ivfflat_candidate_selection_over_the_memory_budget() {
    // Arrange
    let records = vec![record("one", DistanceMetric::L2, &[1.0, 0.0])];

    // Act
    let result = rank_ivfflat_candidate_ids(records, &[1.0, 0.0], 1, &controls(1));

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
}

#[test]
fn should_select_the_nearest_candidate_from_the_probed_ivfflat_lists() {
    // Arrange
    let (cassie, spec) = ivfflat_hybrid_fixture(true);
    let retrieval_controls = controls(4096);
    let request = retrieval_controls.storage_request();
    let (_, training, _, _, _) = cassie
        .midge
        .get_ivfflat_training_manifest_controlled("ivf_rank", "embedding", &request)
        .expect("read IVFFlat training state")
        .expect("trained IVFFlat state")
        .into_parts();
    let probed_lists = crate::vector::ivfflat::probe_lists(&[1.0, 0.0], &training);
    let (_, traversed, _, _, _) = cassie
        .midge
        .ivfflat_candidate_vectors_controlled(
            &spec.collection,
            "embedding",
            &training,
            &probed_lists,
            &request,
        )
        .expect("read candidates from probed lists")
        .into_parts();
    let nearest_id = traversed
        .iter()
        .min_by(|left, right| {
            super::ivfflat_candidate_distance(&[1.0, 0.0], left)
                .expect("valid vector")
                .total_cmp(
                    &super::ivfflat_candidate_distance(&[1.0, 0.0], right)
                        .expect("valid vector"),
                )
        })
        .expect("at least one probed vector")
        .id
        .clone();
    let expected = rank_ivfflat_candidate_ids(
        traversed.clone(),
        &[1.0, 0.0],
        1,
        &controls(4096),
    )
    .expect("rank every vector in probed lists")
    .0;
    let standalone = cassie
        .midge
        .persisted_vector_candidate_ids(&spec.collection, "embedding", &[1.0, 0.0], 1)
        .expect("run standalone IVFFlat retrieval")
        .expect("use standalone IVFFlat candidates");

    // Act
    let actual = super::controlled_vector_candidate_ids(&cassie, &spec, 1, &controls(4096))
        .expect("select IVFFlat hybrid candidates")
        .expect("use trained IVFFlat candidates");

    // Assert
    assert_eq!(traversed.len(), 2);
    assert_ne!(
        traversed.first().map(|record| record.id.as_str()),
        Some(nearest_id.as_str()),
        "the nearest vector must occur after the first traversal candidate"
    );
    assert_eq!(
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        [nearest_id.as_str()]
    );
    assert_eq!(standalone, expected);
    assert_eq!(actual.ids, expected);
}

#[test]
fn should_keep_exact_hybrid_fallback_when_no_vector_index_exists() {
    // Arrange
    let (cassie, spec) = ivfflat_hybrid_fixture(false);

    // Act
    let selected = super::controlled_vector_candidate_ids(&cassie, &spec, 1, &controls(4096))
        .expect("missing vector index selects exact fallback");

    // Assert
    assert!(selected.is_none());
}

#[test]
fn should_skip_non_l2_ivfflat_index_candidates_for_hybrid_exact_fallback() {
    // Arrange
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.embeddings =
        crate::config::EmbeddingsRuntimeConfig::Local(crate::config::LocalRuntimeConfig {
            model: "hybrid-test".to_string(),
            dimensions: 2,
        });
    let cassie = crate::app::Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, "CREATE EXTENSION vector", Vec::new())
        .expect("install vector extension");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE metric_mismatch (id BIGINT PRIMARY KEY, body TEXT, embedding VECTOR(2))",
            Vec::new(),
        )
        .expect("create mismatch table");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX metric_mismatch_embedding_idx ON metric_mismatch \
                 USING vector (embedding) WITH (source_field = body, metric = dot, \
                 index_type = ivfflat, lists = 1, probes = 1, \
                 training_sample_size = 1, training_seed = 17)",
            Vec::new(),
        )
        .expect("create mismatch index");
    let spec = super::super::HybridTopKSpec {
        collection: crate::catalog::canonical_relation_name(
            &cassie.default_database,
            crate::catalog::DEFAULT_SCHEMA,
            "metric_mismatch",
        ),
        text_field: "body".to_string(),
        query: "match".to_string(),
        vector_field: "embedding".to_string(),
        vector_query: vec![1.0, 0.0],
        filter: None,
        id_column: "id".to_string(),
        score_column: "score".to_string(),
        limit: 1,
        offset: 0,
    };

    // Act
    let selected = super::controlled_vector_candidate_ids(&cassie, &spec, 1, &controls(4096))
        .expect("non-L2 index selects exact fallback");

    // Assert
    assert!(selected.is_none());
}

#[test]
fn should_skip_non_l2_hnsw_index_candidates_for_hybrid_exact_fallback() {
    // Arrange
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.embeddings =
        crate::config::EmbeddingsRuntimeConfig::Local(crate::config::LocalRuntimeConfig {
            model: "hybrid-test".to_string(),
            dimensions: 2,
        });
    let cassie = crate::app::Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, "CREATE EXTENSION vector", Vec::new())
        .expect("install vector extension");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE metric_mismatch (id BIGINT PRIMARY KEY, body TEXT, embedding VECTOR(2))",
            Vec::new(),
        )
        .expect("create mismatch table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO metric_mismatch (id, body, embedding) VALUES \
                 (1, 'near', '[1,0]'), (2, 'mid', '[2,1]'), (3, 'far', '[-1,0]')",
            Vec::new(),
        )
        .expect("populate mismatch table");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX metric_mismatch_embedding_idx ON metric_mismatch \
                 USING vector (embedding) WITH (source_field = body, metric = dot, \
                 index_type = hnsw, m = 4, ef_construction = 8, ef_search = 8)",
            Vec::new(),
        )
        .expect("create mismatch index");
    let collection = crate::catalog::canonical_relation_name(
        &cassie.default_database,
        crate::catalog::DEFAULT_SCHEMA,
        "metric_mismatch",
    );
    let index = cassie
        .midge
        .get_vector_index(&collection, "embedding")
        .expect("load persisted mismatch index")
        .expect("persisted mismatch index");
    let graph = index
        .metadata
        .hnsw_graph
        .as_ref()
        .expect("persisted HNSW graph");
    assert_eq!(graph.row_count, 3);
    assert!(graph.entry_point.is_some());
    let ann = cassie
        .midge
        .search_hnsw_graph_point_read(
            &collection,
            "embedding",
            &[1.0, 0.0],
            index.metadata.hnsw.as_ref().expect("HNSW options"),
            1,
        )
        .expect("read persisted HNSW graph")
        .expect("usable HNSW graph");
    assert_ne!(ann.candidates.len(), 0);
    let spec = super::super::HybridTopKSpec {
        collection,
        text_field: "body".to_string(),
        query: "match".to_string(),
        vector_field: "embedding".to_string(),
        vector_query: vec![1.0, 0.0],
        filter: None,
        id_column: "id".to_string(),
        score_column: "score".to_string(),
        limit: 1,
        offset: 0,
    };

    // Act
    let selected = super::controlled_vector_candidate_ids(&cassie, &spec, 1, &controls(4096))
        .expect("non-L2 index selects exact fallback");

    // Assert
    assert!(selected.is_none());
}
