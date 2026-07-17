use std::sync::{Arc, Barrier};

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};

#[path = "support/sql.rs"]
mod support;

const TABLE: &str = "ann_concurrent_source";
const QUERY: &str = "SELECT id, vector_distance(embedding, '[0,0,0]') AS distance FROM ann_concurrent_source ORDER BY distance ASC LIMIT 5";

fn fixture(path: &str, index_type: &str) -> Arc<Cassie> {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    let cassie = Arc::new(
        Cassie::new_with_data_dir_and_config(path, config).expect("create concurrent ANN fixture"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ann_concurrent_source (content TEXT, embedding VECTOR(3))",
            vec![],
        )
        .expect("create table");
    let documents = (0..32)
        .map(|index| {
            let coordinate = index.to_string().parse::<f64>().expect("coordinate") / 100.0;
            (
                Some(format!("row-{index:04}")),
                serde_json::json!({
                    "content": format!("row-{index:04}"),
                    "embedding": [coordinate, coordinate / 2.0, 0.0]
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(TABLE, documents)
        .expect("seed rows");
    let options = match index_type {
        "hnsw" => "index_type = hnsw, m = 8, ef_construction = 64, ef_search = 32",
        "ivfflat" => "index_type = ivfflat, lists = 4, probes = 4, training_sample_size = 32, training_seed = 7",
        _ => panic!("unsupported fixture index type"),
    };
    cassie
        .execute_sql(
            &session,
            &format!("CREATE INDEX ann_concurrent_vector ON ann_concurrent_source USING vector (embedding) WITH (source_field = content, metric = l2, {options})"),
            vec![],
        )
        .expect("create HNSW index");
    cassie
}

#[test]
fn should_discard_hnsw_attempt_when_source_changes_before_reranking() {
    // Arrange
    support::with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = support::data_dir("ann-concurrent-source");
    let cassie = fixture(&path, "hnsw");
    let selected = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    cassie::executor::set_vector_ann_rerank_barriers(
        Some(Arc::clone(&selected)),
        Some(Arc::clone(&resume)),
    );
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        query_cassie
            .execute_sql(&query_cassie.create_session("reader", None), QUERY, vec![])
            .expect("concurrent ANN query")
    });
    selected.wait();

    // Act
    cassie
        .execute_sql(
            &cassie.create_session("writer", None),
            "DELETE FROM ann_concurrent_source WHERE id = 'row-0000'",
            vec![],
        )
        .expect("delete selected source row");
    resume.wait();
    let resolved = query.join().expect("query thread");
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "DROP INDEX ann_concurrent_vector ON ann_concurrent_source",
            vec![],
        )
        .expect("drop ANN index for exact baseline");
    let exact = cassie
        .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
        .expect("exact baseline");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(resolved.rows, exact.rows);
    assert_eq!(
        metrics["vector"]["last_fallback_reason"].as_str(),
        Some("concurrent-source-change")
    );
    assert_eq!(metrics["vector"]["hnsw_executions"].as_u64(), Some(0));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_discard_ivfflat_attempt_when_source_is_replaced_before_reranking() {
    // Arrange
    support::with_fallback();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = support::data_dir("ivfflat-concurrent-source");
    let cassie = fixture(&path, "ivfflat");
    let selected = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    cassie::executor::set_vector_ann_rerank_barriers(
        Some(Arc::clone(&selected)),
        Some(Arc::clone(&resume)),
    );
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        query_cassie
            .execute_sql(&query_cassie.create_session("reader", None), QUERY, vec![])
            .expect("concurrent IVFFlat query")
    });
    selected.wait();

    // Act
    cassie
        .execute_sql(
            &cassie.create_session("writer", None),
            "UPDATE ann_concurrent_source SET embedding = '[9,9,9]' WHERE id = 'row-0000'",
            vec![],
        )
        .expect("replace selected source vector");
    resume.wait();
    let resolved = query.join().expect("query thread");
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "DROP INDEX ann_concurrent_vector ON ann_concurrent_source",
            vec![],
        )
        .expect("drop ANN index for exact baseline");
    let exact = cassie
        .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
        .expect("exact baseline");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(resolved.rows, exact.rows);
    assert_eq!(
        metrics["vector"]["last_fallback_reason"].as_str(),
        Some("concurrent-source-change")
    );
    assert_eq!(metrics["vector"]["ivfflat_executions"].as_u64(), Some(0));
    let _ = std::fs::remove_dir_all(path);
}
