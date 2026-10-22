use cassie::app::{Cassie, CassieError};
use cassie::config::{
    CassieRuntimeConfig, CassieRuntimeLimits, EmbeddingsRuntimeConfig, LocalRuntimeConfig,
};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};

#[test]
fn should_reserve_cold_normalized_decode_overlap_before_the_next_read() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let config = CassieRuntimeConfig {
        embeddings: EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "normalized-controls".to_owned(),
            dimensions: 4096,
        }),
        limits: CassieRuntimeLimits {
            query_memory_budget_bytes: 40 * 1024,
            ..CassieRuntimeLimits::default()
        },
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("engine");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE normalized_overlap (content TEXT, embedding VECTOR(4096))",
            vec![],
        )
        .expect("table");
    cassie
        .midge
        .put_fresh_documents(
            "normalized_overlap",
            (0..2)
                .map(|index| {
                    (
                        Some(format!("row-{index}")),
                        serde_json::json!({
                            "content": "query", "embedding": vec![1.0; 4096]
                        }),
                    )
                })
                .collect(),
        )
        .expect("vectors");
    cassie.execute_sql(&session,
            "CREATE INDEX normalized_overlap_idx ON normalized_overlap USING vector (embedding) WITH (source_field = content, metric = cosine, index_type = bruteforce)", vec![])
            .expect("index");
    let before = cassie.metrics();
    let before_reads = cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(2));

    // Act
    let result = cassie::rest::search::vector_search(
        &cassie,
        "normalized_overlap",
        br#"{"field":"embedding","query":"query","metric":"cosine","limit":1}"#,
    );
    set_query_scan_cancellation_after_entries(None);
    let after = cassie.metrics();
    let error = result.expect_err("query must reject the first record overlap");

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)), "{error:?}");
    assert_eq!(
        cassie.midge.query_scan_entries_for_diagnostics() - before_reads,
        1
    );
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["vector"]["normalized_candidate_count_total"],
        before["vector"]["normalized_candidate_count_total"]
    );
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    drop(cassie);
}
