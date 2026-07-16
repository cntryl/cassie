use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::runtime::QueryCancellationHandle;
use cassie::types::{Value, Vector};

#[path = "support/sql.rs"]
mod support;
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
                Some(content.clone()),
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

#[test]
fn should_match_exact_top_k_with_hnsw_for_bound_vector_parameters() {
    // Arrange
    with_fallback();
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
    assert_eq!(indexed.rows, exact.rows);
    assert!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            > before["vector"]["hnsw_executions"].as_u64().unwrap()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reach_ivfflat_recall_threshold_against_exact_top_k() {
    // Arrange
    with_fallback();
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
    let exact_ids = result_ids(&exact.rows).into_iter().collect::<HashSet<_>>();
    let overlap = result_ids(&indexed.rows)
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
    with_fallback();
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
    assert!(!result_ids(&result.rows).contains(&"row-0000".to_string()));
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
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_exact_memory_budget");
    let cassie = vector_cassie_with_memory_budget(&path, 32);
    let session = cassie.create_session("tester", None);
    let collection = "vector_exact_memory_budget";
    seed_vector_collection(&cassie, collection, 20);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
        )
        .expect_err("exact vector heap should exceed the memory budget");

    // Assert
    assert!(
        error.to_string().contains("query memory budget"),
        "unexpected error: {error}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cancel_during_exact_vector_scoring() {
    // Arrange
    with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = data_dir("vector_exact_active_cancellation");
    let cassie = Arc::new(vector_cassie(&path));
    let collection = "vector_exact_active_cancellation";
    seed_vector_collection(&cassie, collection, 100_000);
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        let session = query_cassie.create_session("tester", None);
        query_cassie.execute_sql_with_cancellation(
            &session,
            &vector_query(collection, false),
            query_params(true, false),
            &query_cancellation,
        )
    });
    std::thread::sleep(Duration::from_millis(5));

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("query thread")
        .expect_err("active vector query should be cancelled");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
