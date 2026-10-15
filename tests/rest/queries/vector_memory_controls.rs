use super::support_retrieval::{
    circle_fixture, circle_fixture_with_row_limit, create_index, open_circle_fixture,
};
use super::support_retrieval_provider::CountingEmbeddingProvider;
use cassie::app::CassieError;
use cassie::embeddings::EmbeddingProvider;

#[test]
fn should_reject_rest_vector_search_under_the_same_budget_as_sql() {
    // Arrange
    super::support_sql::use_local_storage();
    let (setup, path) = circle_fixture("rest_vector_budget", 64, 64 * 1024);
    create_index(&setup, "index_type = bruteforce");
    drop(setup);
    let cassie = open_circle_fixture(&path, 1, 100_000);
    cassie.startup().expect("hydrate persisted vector fixture");
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();

    // Act
    let sql = cassie.execute_sql(&session, "SELECT id, vector_distance(embedding, '[1,0,1]') AS distance FROM docs ORDER BY distance ASC LIMIT 10", vec![]);
    let rest = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"l2","limit":10}"#,
    );
    let after = cassie.metrics();

    // Assert
    assert!(matches!(sql, Err(CassieError::ResourceLimit(_))));
    assert!(
        matches!(rest, Err(CassieError::ResourceLimit(_))),
        "REST must reject the shared budget"
    );
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_preserve_zero_limit_with_a_tiny_rest_vector_budget() {
    // Arrange
    super::support_sql::use_local_storage();
    let (setup, path) = circle_fixture("rest_vector_zero_budget", 3, 64 * 1024);
    create_index(&setup, "index_type = bruteforce");
    drop(setup);
    let mut denied = open_circle_fixture(&path, 1, 100_000);
    denied.startup().expect("hydrate one-byte vector fixture");
    let denied_provider = CountingEmbeddingProvider::install(&mut denied);
    drop(
        denied_provider
            .embed_query("query")
            .expect("provider control"),
    );
    let denied_counter_control = denied_provider.calls();
    denied_provider.reset_calls();
    let denied_before = denied.metrics();
    let denied_scan_before = denied.midge.query_scan_entries_for_diagnostics();

    // Act
    let rejected = matches!(
        cassie::rest::search::vector_search(
            &denied,
            "docs",
            br#"{"field":"embedding","query":"query","limit":0}"#,
        ),
        Err(CassieError::ResourceLimit(_))
    );
    let denied_after = denied.metrics();
    let denied_scan_after = denied.midge.query_scan_entries_for_diagnostics();
    let denied_provider_calls = denied_provider.calls();
    drop(denied_provider);
    drop(denied);

    // The same zero-row request still owns metadata and a managed REST payload.
    // A separate small admitted budget qualifies empty success without vector work.
    let mut cassie = open_circle_fixture(&path, 4 * 1024, 100_000);
    cassie.startup().expect("hydrate admitted vector fixture");
    let provider = CountingEmbeddingProvider::install(&mut cassie);
    drop(provider.embed_query("query").expect("provider control"));
    let counter_control = provider.calls();
    provider.reset_calls();
    let before = cassie.metrics();
    let scan_before = cassie.midge.query_scan_entries_for_diagnostics();
    let result = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","limit":0}"#,
    );
    let output_error = result.as_ref().err().map(ToString::to_string);
    let empty = result.as_ref().is_ok_and(|result| result.rows().is_empty());
    let serializable = result
        .as_ref()
        .is_ok_and(|result| serde_json::to_value(result).is_ok());
    let live = cassie.metrics();
    let scan_after = cassie.midge.query_scan_entries_for_diagnostics();
    let provider_calls = provider.calls();
    drop(result);
    let retired = cassie.metrics();
    drop(provider);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");

    // Assert
    assert!(rejected, "one byte cannot admit the required empty output");
    assert_eq!(denied_counter_control, 1);
    assert_eq!(denied_provider_calls, 0);
    assert_eq!(denied_scan_after, denied_scan_before);
    assert_eq!(denied_after["vector"], denied_before["vector"]);
    assert_eq!(denied_after["query"]["current_accounted_memory_bytes"], 0);
    assert!(
        empty,
        "an admitted zero limit returns no rows: {output_error:?}"
    );
    assert!(
        serializable,
        "empty managed REST output remains serializable"
    );
    assert_eq!(counter_control, 1);
    assert_eq!(provider_calls, 0);
    assert_eq!(scan_after, scan_before);
    assert_eq!(live["vector"], before["vector"]);
    assert!(
        live["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("live empty REST result memory")
            > 0
    );
    assert_eq!(retired["query"]["current_accounted_memory_bytes"], 0);
}

#[test]
fn should_account_long_rest_query_cache_keys_before_copying_them() {
    // Arrange
    let (cassie, path) = circle_fixture("rest-long-query-keys", 1, 4096);
    create_index(&cassie, "index_type = bruteforce");
    let body = serde_json::to_vec(&serde_json::json!({
        "field": "embedding", "query": "q".repeat(32 * 1024),
        "metric": "l2", "limit": 1
    }))
    .expect("request body");
    let before = cassie.metrics();

    // Act
    let result = cassie::rest::search::vector_search(&cassie, "docs", &body);
    let after = cassie.metrics();

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_reserve_sparse_json_nodes_before_rest_vector_output() {
    // Arrange
    let (cassie, path) = circle_fixture("rest-sparse-json-budget", 1, 16 * 1024);
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "ALTER TABLE docs ADD COLUMN payload JSON",
            vec![],
        )
        .expect("JSON column");
    let payload = (0..32).fold(
        serde_json::Value::Null,
        |value, _| serde_json::json!({"x": value}),
    );
    cassie.midge.put_documents("docs", vec![(Some("row-00".to_owned()),
            serde_json::json!({"content": "query", "embedding": [1.0, 0.0, 1.0], "payload": payload}))])
            .expect("sparse JSON row");
    create_index(&cassie, "index_type = bruteforce");
    let before = cassie.metrics();

    // Act
    let result = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"l2","limit":1}"#,
    );
    let after = cassie.metrics();

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_enforce_actual_rest_vector_result_rows_without_rejecting_a_larger_request() {
    // Arrange
    let (cassie, path) = circle_fixture_with_row_limit("rest-row-limit", 3, 1024 * 1024, 1);
    create_index(&cassie, "index_type = bruteforce");
    let before = cassie.metrics();

    // Act
    let excessive = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"l2","limit":3}"#,
    );
    let after = cassie.metrics();
    let one = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"l2","limit":10,"offset":2}"#,
    )
    .expect("one eligible output row fits the cap");

    // Assert
    assert!(matches!(excessive, Err(CassieError::ResourceLimit(_))));
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(one.rows().len(), 1);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

fn cosine_index(cassie: &cassie::app::Cassie) {
    cassie.execute_sql(&cassie.create_session("tester", None),
            "CREATE INDEX docs_vector ON docs USING vector (embedding) WITH (source_field = content, metric = cosine, index_type = bruteforce)", vec![]).expect("cosine index");
}

fn add_wide_payloads(cassie: &cassie::app::Cassie, rows: u32) {
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "ALTER TABLE docs ADD COLUMN payload TEXT",
            vec![],
        )
        .expect("wide column");
    let documents = (0..rows)
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / f64::from(rows);
            (
                Some(format!("row-{index:02}")),
                serde_json::json!({
                    "content": "query", "embedding": [angle.cos(), angle.sin(), 1.0],
                    "payload": "x".repeat(8192)
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_documents("docs", documents)
        .expect("wide rows");
}

#[test]
fn should_reject_cold_normalized_rest_search_without_success_metrics_given_low_memory() {
    // Arrange
    let (cassie, path) = circle_fixture("rest_normalized_cold_budget", 64, 512);
    cosine_index(&cassie);
    let before = cassie.metrics();

    // Act
    let result = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"cosine","limit":10}"#,
    );
    let after = cassie.metrics();

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["vector"]["normalized_candidate_count_total"],
        before["vector"]["normalized_candidate_count_total"]
    );
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_reject_warm_normalized_rest_output_without_success_metrics_given_low_memory() {
    // Arrange
    // One wide row fits the conservative row-decode estimate; sixty-four do not.
    let (cassie, path) = circle_fixture("rest_normalized_warm_budget", 64, 128 * 1024);
    add_wide_payloads(&cassie, 64);
    cosine_index(&cassie);
    cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"cosine","limit":1}"#,
    )
    .expect("warm normalized cache");
    let before = cassie.metrics();

    // Act
    let result = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"cosine","limit":64}"#,
    );
    let after = cassie.metrics();

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["vector"]["normalized_candidate_count_total"],
        before["vector"]["normalized_candidate_count_total"]
    );
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");
}

#[test]
fn should_reject_rest_ann_output_before_success_given_low_memory() {
    // Arrange
    for index_type in ["hnsw", "ivfflat"] {
        let (cassie, path) = circle_fixture(&format!("rest_{index_type}_output_budget"), 8, 32768);
        add_wide_payloads(&cassie, 8);
        let options = match index_type {
                "hnsw" => "index_type = hnsw, m = 2, ef_construction = 8, ef_search = 8",
                _ => "index_type = ivfflat, lists = 2, probes = 2, training_sample_size = 8, training_seed = 17",
            };
        create_index(&cassie, options);
        let before = cassie.metrics();

        // Act
        let result = cassie::rest::search::vector_search(
            &cassie,
            "docs",
            br#"{"field":"embedding","query":"query","metric":"l2","limit":8}"#,
        );
        let after = cassie.metrics();

        // Assert
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{index_type} must account output"
        );
        assert_eq!(after["vector"]["count"], before["vector"]["count"]);
        assert_eq!(
            after["vector"]["hnsw_executions"],
            before["vector"]["hnsw_executions"]
        );
        assert_eq!(
            after["vector"]["ivfflat_executions"],
            before["vector"]["ivfflat_executions"]
        );
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        drop(cassie);
        std::fs::remove_dir_all(path).expect("cleanup fixture");
    }
}

#[test]
fn should_serialize_finite_rest_vector_rows_from_each_cache_state() {
    // Arrange
    let (cassie, path) = circle_fixture("rest_finite_cached", 1024, 1024 * 1024);
    create_index(&cassie, "index_type = bruteforce");
    let request = br#"{"field":"embedding","query":"query","metric":"l2","limit":10}"#;
    let before = cassie.metrics();

    // Act
    let cold = cassie::rest::search::vector_search(&cassie, "docs", request).expect("cold search");
    let after_cold = cassie.metrics();
    let cached =
        cassie::rest::search::vector_search(&cassie, "docs", request).expect("cached search");
    let metrics = cassie.metrics();
    let row_count = cold.rows().len();
    let matching_rows = cold.rows() == cached.rows();
    let serializable = serde_json::to_value(&cached).is_ok();
    drop(cold);
    drop(cached);
    let after_result_retirement = cassie.metrics();
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");

    // Assert
    assert_eq!(row_count, 10);
    assert_eq!(
        after_cold["vector"]["count"]
            .as_u64()
            .expect("cold executions")
            - before["vector"]["count"]
                .as_u64()
                .expect("initial executions"),
        1,
        "cold L2 request must execute the row scorer exactly once"
    );
    assert_eq!(
        metrics["vector"]["count"], after_cold["vector"]["count"],
        "second L2 request must use the result cache rather than execute the row scorer"
    );
    assert!(matching_rows);
    assert!(serializable);
    assert!(
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("peak")
            > 0
    );
    assert!(
        metrics["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("live result memory")
            > 0
    );
    assert_eq!(
        after_result_retirement["query"]["current_accounted_memory_bytes"], 0,
        "retiring DTOs must release the latest requesting query's owned buffers"
    );
}

#[test]
fn should_preserve_the_local_query_embedding_override_in_controlled_rest_search() {
    use cassie::embeddings::local::{LocalProvider, LocalProviderConfig};
    use cassie::embeddings::EmbeddingProvider;

    // Arrange
    let (cassie, path) = circle_fixture("rest-local-query-equivalence", 2, 1024 * 1024);
    let provider = LocalProvider::with_config(LocalProviderConfig {
        model: "retrieval-controls".to_owned(),
        dimensions: 3,
    })
    .expect("matching local provider");
    let query_embedding = provider.embed_query("query").expect("query embedding");
    let document_embedding = provider
        .embed_documents(&["query".to_owned()])
        .expect("document embedding")
        .pop()
        .expect("one document embedding");
    assert_ne!(query_embedding.values, document_embedding.values);
    cassie
        .midge
        .put_documents(
            "docs",
            vec![
                (
                    Some("row-00".to_owned()),
                    serde_json::json!({
                        "content": "query", "embedding": query_embedding.values
                    }),
                ),
                (
                    Some("row-01".to_owned()),
                    serde_json::json!({
                        "content": "query", "embedding": document_embedding.values
                    }),
                ),
            ],
        )
        .expect("seed distinct query and document embeddings");
    create_index(&cassie, "index_type = bruteforce");

    // Act
    let result = cassie::rest::search::vector_search(
        &cassie,
        "docs",
        br#"{"field":"embedding","query":"query","metric":"l2","limit":1}"#,
    )
    .expect("controlled local search");
    let matching_row = result.rows()[0][0] == serde_json::json!("row-00");
    let live = cassie.metrics();
    drop(result);
    let retired = cassie.metrics();
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup fixture");

    // Assert
    assert!(matching_row);
    assert!(
        live["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("live result memory")
            > 0
    );
    assert_eq!(retired["query"]["current_accounted_memory_bytes"], 0);
}
