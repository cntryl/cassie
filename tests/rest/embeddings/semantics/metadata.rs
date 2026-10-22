#[test]
fn should_fallback_before_rest_hnsw_search_given_incompatible_graph_metadata() {
    for (mutation, expected_reason) in [
        ("metric", "incompatible-metric"),
        ("version", "unsupported-graph-version"),
        ("dimensions", "incompatible-dimensions"),
    ] {
        // Arrange
        let path = data_dir(&format!("rest-hnsw-metadata-{mutation}"));
        let embedding_server = MockOpenAiServer::spawn(vec![MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        }]);
        let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
        )
        .expect("engine");
        let collection = "rest_hnsw_metadata";
        create_vector_collection(&cassie, collection, 3);
        cassie
            .midge
            .put_fresh_documents(
                &canonical_collection(collection),
                vec![
                    (
                        Some("near".to_owned()),
                        serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
                    ),
                    (
                        Some("far".to_owned()),
                        serde_json::json!({"embedding": [2.0, 0.0, 0.0]}),
                    ),
                ],
            )
            .expect("seed explicit vectors");
        create_vector_index_with_options(
            &cassie,
            collection,
            &serde_json::json!({
                "source_field": "content", "metric": "l2", "index_type": "hnsw",
                "m": "2", "ef_construction": "4", "ef_search": "2"
            }),
        );
        let mut index = cassie
            .midge
            .get_vector_index(&canonical_collection(collection), "embedding")
            .expect("stored index")
            .expect("HNSW index");
        let graph = index.metadata.hnsw_graph.as_mut().expect("graph");
        match mutation {
            "metric" => graph.metric = cassie::embeddings::DistanceMetric::Dot,
            "version" => graph.version = 0,
            "dimensions" => graph.dimensions = 4,
            _ => unreachable!("known graph metadata mutation"),
        }
        cassie
            .midge
            .put_vector_index_state(
                &canonical_collection(collection),
                "embedding",
                cassie::embeddings::VectorIndexState {
                    built_generation: 0,
                    hnsw_graph: index.metadata.hnsw_graph,
                    ivfflat_training: None,
                },
            )
            .expect("change only the graph metadata at the current generation");
        let before = cassie.metrics();

        // Act
        let result = vector_search(&cassie, collection, "l2", 1, 0);
        let after = cassie.metrics();

        // Assert
        assert_eq!(row_ids(&result), vec!["near"], "mutation={mutation}");
        assert_eq!(
            after["vector"]["last_fallback_reason"], expected_reason,
            "mutation={mutation}"
        );
        assert_eq!(
            after["vector"]["hnsw_executions"], before["vector"]["hnsw_executions"],
            "an incompatible graph must not publish successful HNSW metrics"
        );
        assert_eq!(
            after["vector"]["count"].as_u64().expect("vector count")
                - before["vector"]["count"].as_u64().expect("vector count"),
            1,
            "only the winning exact path reports execution"
        );
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        drop(cassie);
        cleanup_path(Path::new(&path));
    }
}
