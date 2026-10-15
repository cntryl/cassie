use super::support_retrieval::{circle_fixture, create_index};
use cassie::types::Value;

fn top_k(
    cassie: &cassie::app::Cassie,
    limit: usize,
    offset: usize,
) -> cassie::executor::QueryResult {
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            &format!(
                "SELECT id, vector_distance(embedding, '[1,0,1]') AS distance FROM docs ORDER BY distance ASC LIMIT {limit} OFFSET {offset}"
            ),
            vec![],
        )
        .expect("execute vector top-k")
}

#[test]
fn should_fallback_when_ivfflat_cannot_fill_the_requested_window() {
    // Arrange
    let (cassie, path) = circle_fixture("ivf_exhaustion", 64, 64 * 1024 * 1024);
    let exact = top_k(&cassie, 10, 0);
    create_index(&cassie, "index_type = ivfflat, lists = 64, probes = 1, training_sample_size = 64, training_seed = 17");
    let before = cassie.metrics();

    // Act
    let indexed = top_k(&cassie, 10, 0);
    let after = cassie.metrics();

    // Assert
    assert_eq!(indexed.rows(), exact.rows());
    assert_eq!(
        after["vector"]["last_fallback_reason"],
        "candidate-exhausted"
    );
    assert_eq!(
        after["vector"]["ivfflat_executions"],
        before["vector"]["ivfflat_executions"]
    );
    assert_eq!(
        after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
            - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
        1
    );
    for metric in [
        "ann_reads_total",
        "exact_reranks_total",
        "retrieval_stage_queries_total",
    ] {
        assert_eq!(
            after["vector"][metric], before["vector"][metric],
            "discarded IVF attempt published {metric}"
        );
    }
    assert_eq!(
        after["vector"]["count"].as_u64().unwrap() - before["vector"]["count"].as_u64().unwrap(),
        1
    );
    drop((indexed, exact));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_fallback_when_ivfflat_candidates_do_not_reach_the_offset() {
    // Arrange
    let (cassie, path) = circle_fixture("ivf_offset_exhaustion", 64, 64 * 1024 * 1024);
    let exact = top_k(&cassie, 4, 8);
    create_index(&cassie, "index_type = ivfflat, lists = 64, probes = 1, training_sample_size = 64, training_seed = 17");

    // Act
    let indexed = top_k(&cassie, 4, 8);

    // Assert
    assert_eq!(indexed.rows(), exact.rows());
    assert_eq!(indexed.rows().len(), 4);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_preserve_small_ivfflat_result_windows() {
    // Arrange
    let (cassie, path) = circle_fixture("ivf_small_window", 3, 64 * 1024 * 1024);
    cassie
        .midge
        .put_fresh_documents(
            "docs",
            vec![(Some("no-vector".to_string()), serde_json::json!({}))],
        )
        .expect("add NULL vector row");
    let exact = top_k(&cassie, 10, 0);
    let exact_offset = top_k(&cassie, 10, 1);
    create_index(
        &cassie,
        "index_type = ivfflat, lists = 2, probes = 2, training_sample_size = 3, training_seed = 17",
    );
    let before = cassie.metrics();

    // Act
    let indexed = top_k(&cassie, 10, 0);
    let indexed_offset = top_k(&cassie, 10, 1);
    let beyond_end = top_k(&cassie, 10, 10);
    let empty = top_k(&cassie, 0, 0);
    let after = cassie.metrics();

    // Assert
    assert_eq!(indexed.rows(), exact.rows());
    assert_eq!(indexed_offset.rows(), exact_offset.rows());
    assert_eq!(beyond_end.rows(), Vec::<Vec<Value>>::new());
    assert_eq!(empty.rows(), Vec::<Vec<Value>>::new());
    assert_eq!(
        after["vector"]["ivfflat_fallbacks"],
        before["vector"]["ivfflat_fallbacks"]
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_bound_hnsw_result_windows_by_eligible_vectors() {
    // Arrange
    let (cassie, path) = circle_fixture("hnsw_eligible_window", 3, 64 * 1024 * 1024);
    cassie
        .midge
        .put_fresh_documents(
            "docs",
            vec![(Some("no-vector".to_string()), serde_json::json!({}))],
        )
        .expect("add NULL vector row");
    let exact = top_k(&cassie, 10, 1);
    create_index(
        &cassie,
        "index_type = hnsw, m = 2, ef_construction = 4, ef_search = 2",
    );
    let before = cassie.metrics();

    // Act
    let indexed = top_k(&cassie, 10, 1);
    let beyond_end = top_k(&cassie, 10, 10);
    let after = cassie.metrics();

    // Assert
    assert_eq!(indexed.rows(), exact.rows());
    assert_eq!(indexed.rows().len(), 2);
    assert_eq!(beyond_end.rows(), Vec::<Vec<Value>>::new());
    assert_eq!(
        after["vector"]["hnsw_fallbacks"],
        before["vector"]["hnsw_fallbacks"]
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_fallback_when_a_disconnected_hnsw_graph_underfills_top_k() {
    // Arrange
    let (cassie, path) = circle_fixture("hnsw_exhaustion", 3, 64 * 1024 * 1024);
    let exact = top_k(&cassie, 2, 0);
    let exact_offset = top_k(&cassie, 1, 1);
    create_index(
        &cassie,
        "index_type = hnsw, m = 2, ef_construction = 4, ef_search = 2",
    );
    let mut index = cassie
        .midge
        .get_vector_index("docs", "embedding")
        .expect("stored index")
        .expect("HNSW index");
    for node in &mut index.metadata.hnsw_graph.as_mut().expect("graph").nodes {
        for neighbors in &mut node.layers {
            neighbors.clear();
        }
    }
    cassie
        .midge
        .put_vector_index_state(
            "docs",
            "embedding",
            cassie::embeddings::VectorIndexState {
                built_generation: 0,
                hnsw_graph: index.metadata.hnsw_graph,
                ivfflat_training: None,
            },
        )
        .expect("disconnect graph without changing source summary");
    let before = cassie.metrics();

    // Act
    let indexed = top_k(&cassie, 2, 0);
    let indexed_offset = top_k(&cassie, 1, 1);
    let after = cassie.metrics();

    // Assert
    assert_eq!(indexed.rows(), exact.rows());
    assert_eq!(indexed_offset.rows(), exact_offset.rows());
    assert_eq!(
        after["vector"]["last_fallback_reason"],
        "candidate-exhausted"
    );
    assert_eq!(
        after["vector"]["hnsw_executions"],
        before["vector"]["hnsw_executions"]
    );
    assert_eq!(
        after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
            - before["vector"]["hnsw_fallbacks"].as_u64().unwrap(),
        2
    );
    for metric in [
        "ann_reads_total",
        "exact_reranks_total",
        "retrieval_stage_queries_total",
    ] {
        assert_eq!(
            after["vector"][metric], before["vector"][metric],
            "discarded HNSW attempt published {metric}"
        );
    }
    drop((indexed, indexed_offset, exact, exact_offset));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}
