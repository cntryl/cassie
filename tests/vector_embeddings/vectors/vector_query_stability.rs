use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::embeddings::{DistanceMetric, HnswIndexOptions, NormalizedVectorRecord};
use cassie::runtime::{QueryCancellationHandle, QueryScoringTestBoundary};
use cassie::types::{Value, Vector};

use super::support_sql as support;
use support::*;

fn vector_cassie(path: &str) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    Cassie::new_with_data_dir_and_config(path, config).expect("create Cassie")
}

fn vector_cassie_with_memory_budget(path: &str, query_memory_budget_bytes: usize) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    Cassie::new_with_data_dir_and_config(path, config).expect("create Cassie")
}

fn seed_vector_collection(cassie: &Cassie, collection: &str, rows: usize) {
    seed_vector_collection_with_first_id(cassie, collection, rows, "row-0000");
}

fn seed_vector_collection_with_first_id(
    cassie: &Cassie,
    collection: &str,
    rows: usize,
    first_id: &str,
) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {collection} (content TEXT, status TEXT, embedding VECTOR(3))"),
            vec![],
        )
        .expect("create vector collection");
    let documents = (0..rows)
        .map(|index| {
            let coordinate = index.to_string().parse::<f64>().expect("f64 index") / 100.0;
            let content = format!("row-{index:04}");
            (
                Some(if index == 0 {
                    first_id.to_owned()
                } else {
                    content.clone()
                }),
                serde_json::json!({
                    "content": content,
                    "status": if index % 2 == 0 { "even" } else { "odd" },
                    "embedding": [coordinate, coordinate / 2.0, 0.0]
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(collection, documents)
        .expect("seed vector rows");
}

fn vector_query(collection: &str, filter: bool) -> String {
    let predicate = if filter { " WHERE status = $2" } else { "" };
    format!(
        "SELECT id, vector_distance(embedding, $1) AS distance FROM {collection}{predicate} ORDER BY distance ASC LIMIT 10"
    )
}

fn query_params(as_vector: bool, filter: bool) -> Vec<Value> {
    let query = if as_vector {
        Value::Vector(Vector::new(vec![0.0, 0.0, 0.0]))
    } else {
        Value::String("[0,0,0]".to_string())
    };
    let mut params = vec![query];
    if filter {
        params.push(Value::String("even".to_string()));
    }
    params
}

fn result_ids(rows: &[Vec<Value>]) -> Vec<String> {
    rows.iter()
        .map(|row| row[0].as_str().expect("row id").to_string())
        .collect()
}

fn seed_large_magnitude_vectors(cassie: &Cassie, collection: &str) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {collection} (content TEXT, embedding VECTOR(3))"),
            vec![],
        )
        .expect("create large-magnitude vector collection");
    let documents = vec![
        (
            Some("large_zero_component".to_string()),
            serde_json::json!({
                "content": "large_zero_component",
                "embedding": [3.0e38, 3.0e38, 0.0]
            }),
        ),
        (
            Some("large_nonzero_component".to_string()),
            serde_json::json!({
                "content": "large_nonzero_component",
                "embedding": [3.0e38, 3.0e38, 3.0e38]
            }),
        ),
    ];
    cassie
        .midge
        .put_fresh_documents(collection, documents)
        .expect("seed large-magnitude vector rows");
}

fn large_magnitude_query_parameters() -> Vec<Value> {
    vec![Value::Vector(Vector::new(vec![
        3.0e38_f32, 3.0e38_f32, 0.0,
    ]))]
}

fn normalized_vector_record(id: &str, vector: &[f32]) -> NormalizedVectorRecord {
    let normalized = cassie::vector::normalize(vector).expect("finite vector should normalize");
    NormalizedVectorRecord {
        collection: "large_magnitude_vectors".to_string(),
        field: "embedding".to_string(),
        id: id.to_string(),
        built_generation: 0,
        dimensions: vector.len(),
        metric: DistanceMetric::L2,
        normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
        payload_available: true,
        magnitude: normalized.magnitude,
        values: normalized.values,
    }
}

fn assert_large_magnitude_index_rows_match_scan(
    exact_rows: &[Vec<Value>],
    indexed_rows: &[Vec<Value>],
) {
    let exact_ids = result_ids(exact_rows);
    assert_eq!(result_ids(indexed_rows), exact_ids);

    let distance_tolerance = 3.0e38_f64.powi(2) * 3.0 * 1.0e-6;
    for (exact_row, indexed_row) in exact_rows.iter().zip(indexed_rows) {
        let exact_distance = exact_row[1]
            .as_f64()
            .expect("scan distance should be a floating-point value");
        let indexed_distance = indexed_row[1]
            .as_f64()
            .expect("indexed distance should be a floating-point value");
        assert!(exact_distance.is_finite());
        assert!(
            indexed_distance.is_finite(),
            "indexed distance must remain finite: {indexed_distance}"
        );
        assert!(
            (exact_distance - indexed_distance).abs() <= distance_tolerance,
            "indexed distance {indexed_distance} differs from scan distance {exact_distance}"
        );
    }
}

#[test]
fn should_reconstruct_ivfflat_vectors_when_magnitude_exceeds_f32_max() {
    // Arrange
    let vector = [3.0e38_f32, 3.0e38_f32, 0.0];
    let record = normalized_vector_record("large_zero_component", &vector);
    assert!(record.magnitude > f64::from(f32::MAX));

    // Act
    let reconstructed = cassie::vector::ivfflat::denormalized_vector(&record)
        .expect("finite vector should reconstruct");

    // Assert
    assert_eq!(reconstructed[2], 0.0);
    for (actual, expected) in reconstructed.iter().zip(vector) {
        assert!(
            actual.is_finite(),
            "reconstructed component {actual} is not finite"
        );
        let relative_error =
            (f64::from(*actual) - f64::from(expected)).abs() / f64::from(expected.abs()).max(1.0);
        assert!(
            relative_error <= 1.0e-6,
            "reconstructed component {actual} differs from {expected}"
        );
    }
}

#[test]
fn should_reconstruct_f32_max_components_after_normalized_rounding() {
    // Arrange
    let vector = [f32::MAX, 1.0e34_f32, 0.0];
    let record = normalized_vector_record("max_component", &vector);
    assert_eq!(record.values[0], 1.0);
    assert!(record.magnitude > f64::from(f32::MAX));

    // Act
    let reconstructed = cassie::vector::ivfflat::denormalized_vector(&record)
        .expect("finite vector should reconstruct at the f32 boundary");

    // Assert
    assert_eq!(reconstructed[0], f32::MAX);
    assert_eq!(reconstructed[2], 0.0);
    assert!(reconstructed.iter().all(|component| component.is_finite()));
}

#[test]
fn should_reject_out_of_range_normalized_vector_components() {
    // Arrange
    let vector = [1.0_f32, 0.0, 0.0];
    let mut record = normalized_vector_record("invalid_metadata", &vector);
    record.values[0] = 1.01;

    // Act
    let invalid_component = cassie::vector::ivfflat::denormalized_vector(&record);

    // Assert
    assert!(invalid_component.is_none());
}

#[test]
fn should_reject_non_finite_normalized_vector_magnitude() {
    // Arrange
    let vector = [1.0_f32, 0.0, 0.0];
    let mut record = normalized_vector_record("invalid_magnitude", &vector);
    record.magnitude = f64::INFINITY;

    // Act
    let invalid_magnitude = cassie::vector::ivfflat::denormalized_vector(&record);

    // Assert
    assert!(invalid_magnitude.is_none());
}

#[test]
fn should_return_finite_hnsw_self_distance_when_magnitude_exceeds_f32_max() {
    // Arrange
    let vector = [3.0e38_f32, 3.0e38_f32, 0.0];
    let record = normalized_vector_record("large_zero_component", &vector);
    assert!(record.magnitude > f64::from(f32::MAX));
    let options = HnswIndexOptions::default();
    let graph =
        cassie::vector::hnsw::build_graph(vec![record], &options, vector.len(), DistanceMetric::L2);

    // Act
    let result = cassie::vector::hnsw::search_graph(&graph, &vector, &options, 1)
        .expect("HNSW should return the one-node result");

    // Assert
    let candidate = result.candidates.first().expect("one HNSW candidate");
    assert_eq!(candidate.id, "large_zero_component");
    assert_eq!(
        candidate.distance,
        cassie::vector::l2_distance(&vector, &vector)
    );
}

#[test]
fn should_return_finite_hnsw_distance_for_f32_max_after_normalized_rounding() {
    // Arrange
    let vector = [-f32::MAX, 1.0e34_f32, 0.0];
    let record = normalized_vector_record("max_component", &vector);
    assert_eq!(record.values[0], -1.0);
    assert!(record.magnitude > f64::from(f32::MAX));
    let options = HnswIndexOptions::default();
    let graph =
        cassie::vector::hnsw::build_graph(vec![record], &options, vector.len(), DistanceMetric::L2);

    // Act
    let result = cassie::vector::hnsw::search_graph(&graph, &vector, &options, 1)
        .expect("HNSW should return the one-node result");

    // Assert
    let candidate = result.candidates.first().expect("one HNSW candidate");
    assert_eq!(candidate.id, "max_component");
    assert_eq!(candidate.distance, 0.0);
}

#[test]
fn should_match_exact_top_k_with_hnsw_for_bound_vector_parameters() {
    // Arrange
    use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_hnsw_exact_baseline");
    let cassie = vector_cassie(&path);
    let session = cassie.create_session("tester", None);
    let collection = "vector_hnsw_exact_baseline";
    seed_vector_collection(&cassie, collection, 200);
    let sql = vector_query(collection, false);
    let exact = cassie
        .execute_sql(&session, &sql, query_params(true, false))
        .expect("exact vector query");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_hnsw_exact_idx ON vector_hnsw_exact_baseline USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 12, ef_construction = 96, ef_search = 64)",
            vec![],
        )
        .expect("create HNSW index");
    let before = cassie.metrics();

    // Act
    let indexed = cassie
        .execute_sql(&session, &sql, query_params(false, false))
        .expect("indexed HNSW query");
    let after = cassie.metrics();

    // Assert
    assert_eq!(indexed.rows(), exact.rows());
    assert!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            > before["vector"]["hnsw_executions"].as_u64().unwrap()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reach_ivfflat_recall_threshold_against_exact_top_k() {
    // Arrange
    use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_ivfflat_recall_baseline");
    let cassie = vector_cassie(&path);
    let session = cassie.create_session("tester", None);
    let collection = "vector_ivfflat_recall_baseline";
    seed_vector_collection(&cassie, collection, 1_000);
    let sql = vector_query(collection, false);
    let exact = cassie
        .execute_sql(&session, &sql, query_params(true, false))
        .expect("exact vector query");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_ivfflat_recall_idx ON vector_ivfflat_recall_baseline USING vector (embedding) WITH (source_field = content, metric = l2, index_type = ivfflat, lists = 16, probes = 8, training_sample_size = 1000, training_seed = 17)",
            vec![],
        )
        .expect("create IVFFlat index");

    // Act
    let indexed = cassie
        .execute_sql(&session, &sql, query_params(false, false))
        .expect("indexed IVFFlat query");

    // Assert
    let exact_ids = result_ids(exact.rows()).into_iter().collect::<HashSet<_>>();
    let overlap = result_ids(indexed.rows())
        .into_iter()
        .filter(|id| exact_ids.contains(id))
        .count();
    assert!(
        overlap >= 9,
        "expected recall@10 >= 0.90, overlap={overlap}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_explicit_exact_fallback_for_filtered_hnsw_after_delete() {
    // Arrange
    use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_hnsw_filtered_fallback");
    let cassie = vector_cassie(&path);
    let session = cassie.create_session("tester", None);
    let collection = "vector_hnsw_filtered_fallback";
    seed_vector_collection(&cassie, collection, 100);
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_hnsw_filtered_idx ON vector_hnsw_filtered_fallback USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 8, ef_construction = 64, ef_search = 32)",
            vec![],
        )
        .expect("create HNSW index");
    cassie
        .execute_sql(
            &session,
            "DELETE FROM vector_hnsw_filtered_fallback WHERE content = $1",
            vec![Value::String("row-0000".to_string())],
        )
        .expect("delete nearest row");
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &vector_query(collection, true),
            query_params(true, true),
        )
        .expect("filtered exact fallback query");
    let after = cassie.metrics();

    // Assert
    assert!(!result_ids(result.rows()).contains(&"row-0000".to_string()));
    assert_eq!(
        after["vector"]["last_fallback_reason"].as_str(),
        Some("structured-filter-exact")
    );
    assert!(
        after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
            > before["vector"]["hnsw_fallbacks"].as_u64().unwrap()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_query_memory_budget_during_exact_vector_top_k() {
    // Arrange
    use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_exact_memory_budget");
    let cassie = vector_cassie_with_memory_budget(&path, 64 * 1_024);
    let collection = "vector_exact_memory_budget";
    // Only the first storage identity changes; the twenty payloads and scores remain.
    // Its lexically first raw key alone exceeds the deeper target admission budget.
    seed_vector_collection_with_first_id(&cassie, collection, 20, &"0".repeat(4_096));
    drop(cassie);
    let mut observations = Vec::new();

    // Act
    for (budget, expected_visited) in [(32, 0), (2 * 1_024, 1)] {
        let cassie = vector_cassie_with_memory_budget(&path, budget);
        cassie.startup().expect("reopen seeded vector collection");
        let session = cassie.create_session("tester", None);
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        let before = cassie.metrics();
        let outcome = cassie.execute_sql(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
        );
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);
        let after = cassie.metrics();
        let error = outcome.err();
        drop(session);
        drop(cassie);
        observations.push((budget, expected_visited, visited, error, before, after));
    }
    let cleanup = std::fs::remove_dir_all(path);

    // Assert
    for (budget, expected_visited, visited, error, before, after) in observations {
        let error = error.expect("exact vector query must reject its retained frontier");
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert!(
            error.to_string().contains("query memory budget"),
            "unexpected error at budget{budget}: {error}"
        );
        assert_eq!(
                visited, expected_visited,
                "budget{budget}: statement-owner denial precedes scanning; deeper denial enters the exact vector source"
            );
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        assert_eq!(after["runtime"]["running_queries"], 0);
        assert_eq!(after["runtime"]["active_operator_workers"], 0);
        assert_eq!(after["vector"]["count"], before["vector"]["count"]);
        assert_eq!(
            after["query"]["rows_returned_total"],
            before["query"]["rows_returned_total"]
        );
    }
    cleanup.expect("retire vector fixture");
}

#[test]
fn should_cancel_during_exact_vector_scoring() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config).expect("create Cassie"));
    let collection = "vector_exact_active_cancellation";
    seed_vector_collection(&cassie, collection, 3);
    let before = cassie.metrics();
    let mut cancellation = QueryCancellationHandle::new();
    let (selected, resume) =
        cancellation.pause_after_scored_candidate(QueryScoringTestBoundary::ExactVector);
    let query_cancellation = cancellation.clone();
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        let session = query_cassie.create_session("tester", None);
        let result = query_cassie.execute_sql_with_cancellation(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
            &query_cancellation,
        );
        let after = query_cassie.metrics();
        let recovery = query_cassie.execute_sql(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
        );
        (result, after, recovery)
    });

    // Act
    let reached = selected.recv_timeout(Duration::from_secs(10));
    let cancelled_before_boundary = cancellation.is_cancelled();
    cancellation.cancel();
    let _ = resume.send(());
    let (result, after, recovery) = query.join().expect("query thread");

    // Assert
    reached.expect("exact vector candidate was scored and retained");
    assert!(!cancelled_before_boundary);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(after["runtime"]["running_queries"], 0);
    assert_eq!(after["runtime"]["active_operator_workers"], 0);
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["query"]["rows_returned_total"],
        before["query"]["rows_returned_total"]
    );
    assert_eq!(recovery.expect("same-session recovery").rows().len(), 3);
}

#[test]
fn should_release_scoring_pause_when_a_diagnostic_endpoint_disconnects() {
    for disconnect_observer in [true, false] {
        // Arrange
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled =
            cassie::config::ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
        let collection = "vector_disconnected_scoring_pause";
        seed_vector_collection(&cassie, collection, 3);
        let session = cassie.create_session("tester", None);
        let mut cancellation = QueryCancellationHandle::new();
        let (selected, resume) =
            cancellation.pause_after_scored_candidate(QueryScoringTestBoundary::ExactVector);
        if disconnect_observer {
            drop(selected);
        } else {
            drop(resume);
        }

        // Act
        let result = cassie.execute_sql_with_cancellation(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
            &cancellation,
        );
        let after = cassie.metrics();
        let recovery = cassie.execute_sql_with_cancellation(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
            &cancellation,
        );

        // Assert
        let error = result.expect_err("disconnected diagnostic must terminate the query");
        assert!(error.to_string().contains("scoring test"), "{error}");
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        assert_eq!(after["runtime"]["running_queries"], 0);
        assert!(!cancellation.is_cancelled());
        assert_eq!(
            recovery
                .expect("consumed pause permits recovery")
                .rows()
                .len(),
            3
        );
    }
}

#[test]
fn should_preserve_finite_l2_distances_for_ivfflat_magnitudes_above_f32_max() {
    // Arrange
    use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_ivfflat_magnitude_range");
    let cassie = vector_cassie(&path);
    let session = cassie.create_session("tester", None);
    let collection = "vector_ivfflat_magnitude_range";
    seed_large_magnitude_vectors(&cassie, collection);
    let normalized = cassie::vector::normalize(&[3.0e38_f32, 3.0e38_f32, 0.0])
        .expect("large finite vector should normalize");
    assert!(normalized.magnitude > f64::from(f32::MAX));
    let sql = format!(
            "SELECT id, vector_distance(embedding, $1) AS distance FROM {collection} ORDER BY distance ASC LIMIT 2"
        );
    let exact = cassie
        .execute_sql(&session, &sql, large_magnitude_query_parameters())
        .expect("exact vector scan");
    cassie
            .execute_sql(
                &session,
                "CREATE INDEX vector_ivfflat_magnitude_range_idx ON vector_ivfflat_magnitude_range USING vector (embedding) WITH (source_field = content, metric = l2, index_type = ivfflat, lists = 1, probes = 1, training_sample_size = 2, training_seed = 17)",
                vec![],
            )
            .expect("create IVFFlat index");
    let before = cassie.metrics();

    // Act
    let indexed = cassie
        .execute_sql(&session, &sql, large_magnitude_query_parameters())
        .expect("IVFFlat vector query");
    let after = cassie.metrics();

    // Assert
    assert!(
        after["vector"]["ivfflat_executions"].as_u64().unwrap()
            > before["vector"]["ivfflat_executions"].as_u64().unwrap()
    );
    assert_large_magnitude_index_rows_match_scan(exact.rows(), indexed.rows());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_finite_l2_distances_for_hnsw_magnitudes_above_f32_max() {
    // Arrange
    use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_hnsw_magnitude_range");
    let cassie = vector_cassie(&path);
    let session = cassie.create_session("tester", None);
    let collection = "vector_hnsw_magnitude_range";
    seed_large_magnitude_vectors(&cassie, collection);
    let normalized = cassie::vector::normalize(&[3.0e38_f32, 3.0e38_f32, 0.0])
        .expect("large finite vector should normalize");
    assert!(normalized.magnitude > f64::from(f32::MAX));
    let sql = format!(
            "SELECT id, vector_distance(embedding, $1) AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"
        );
    let exact = cassie
        .execute_sql(&session, &sql, large_magnitude_query_parameters())
        .expect("exact vector scan");
    cassie
            .execute_sql(
                &session,
                "CREATE INDEX vector_hnsw_magnitude_range_idx ON vector_hnsw_magnitude_range USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 8, ef_construction = 64, ef_search = 32)",
                vec![],
            )
            .expect("create HNSW index");
    let before = cassie.metrics();

    // Act
    let indexed = cassie
        .execute_sql(&session, &sql, large_magnitude_query_parameters())
        .expect("HNSW vector query");
    let after = cassie.metrics();

    // Assert
    assert!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            > before["vector"]["hnsw_executions"].as_u64().unwrap()
    );
    assert_large_magnitude_index_rows_match_scan(exact.rows(), indexed.rows());
    let _ = std::fs::remove_dir_all(path);
}
