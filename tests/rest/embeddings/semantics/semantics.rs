#[test]
fn should_search_vector_docs_after_ingest() {
    // Arrange
    let path = data_dir("search_flow");
    let path_for_cleanup = path.clone();

    let openai_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: response_body(&[{ vec![0.0; 1536] }]),
        },
        MockResponse {
            status: 200,
            body: response_body(&[{
                let mut vector = vec![0.0; 1536];
                vector[0] = 5.0;
                vector
            }]),
        },
        MockResponse {
            status: 200,
            body: response_body(&[{
                let mut vector = vec![0.0; 1536];
                vector[0] = 2.0;
                vector
            }]),
        },
    ]);

    let server_base_url = openai_server.base_url();
    let cassie =
        Cassie::new_in_memory_with_config(openai_runtime_with_server(server_base_url))
            .unwrap();

    cassie.startup().unwrap();
    create_vector_collection(&cassie, "search_collection", 1536);
    create_vector_index(&cassie, "search_collection", "l2");
    let first_id = create_labelled_document(&cassie, "search_collection", "alpha", "first");
    let second_id = create_labelled_document(&cassie, "search_collection", "beta", "second");

    // Act
    let search = vector_search(&cassie, "search_collection", "l2", 2, 0);

    // Assert
    assert_eq!(row_ids(&search), vec![first_id, second_id]);

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_search_vector_docs_with_tei_provider() {
    // Arrange
    let path = data_dir("tei_search_flow");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![5.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 0.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

    // Act
    let rows = search_self_hosted_vector_docs(&cassie, "tei_search_collection");
    // Assert
    assert_eq!(rows.len(), 2);

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_apply_vector_search_offset_after_distance_ordering() {
    // Arrange
    let path = data_dir("vector_search_offset");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![5.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![3.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 0.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();
    create_vector_collection(&cassie, "vector_offset_collection", 3);
    create_vector_index(&cassie, "vector_offset_collection", "l2");
    let _ = create_labelled_document(&cassie, "vector_offset_collection", "far", "third");
    let nearest_id = create_labelled_document(&cassie, "vector_offset_collection", "near", "first");
    let middle_id =
        create_labelled_document(&cassie, "vector_offset_collection", "middle", "second");

    // Act
    let search = vector_search(&cassie, "vector_offset_collection", "l2", 1, 1);

    // Assert
    let returned_id = row_ids(&search).into_iter().next().expect("offset row");
    assert_ne!(returned_id, nearest_id);
    assert_eq!(returned_id, middle_id);

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_exclude_documents_without_vectors_from_rest_top_k() {
    // Arrange
    let path = data_dir("rest_topk_missing_vector");
    let path_for_cleanup = path.clone();
    let openai_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: response_body(&[{
                let mut vector = vec![0.0; 1536];
                vector[0] = 1.0;
                vector
            }]),
        },
        MockResponse {
            status: 200,
            body: response_body(&[{
                let mut vector = vec![0.0; 1536];
                vector[0] = 0.9;
                vector
            }]),
        },
    ]);
    let cassie = Cassie::new_in_memory_with_config(openai_runtime_with_server(openai_server.base_url()),
    )
    .expect("cassie");
    cassie.startup().expect("startup");
    let collection = "rest_topk_missing_vector";
    create_vector_collection(&cassie, collection, 1536);
    create_vector_index(&cassie, collection, "l2");
    let missing = rest::documents::create(
        &cassie,
        collection,
        serde_json::json!({"label": "missing"})
            .to_string()
            .as_bytes(),
    )
    .expect("create document without source text");
    let missing_id = missing["id"]
        .as_str()
        .expect("missing-vector document id")
        .to_string();
    let scored = create_labelled_document(&cassie, collection, "scored", "present");

    // Act
    let result = vector_search(&cassie, collection, "l2", 2, 0);

    // Assert
    let result_ids = row_ids(&result);
    assert_eq!(result_ids, vec![scored]);
    assert!(!result_ids.contains(&missing_id));
    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_fall_back_to_raw_vector_search_when_normalized_sidecars_are_missing() {
    // Arrange
    let path = data_dir("vector_search_normalized_fallback");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![3.0, 4.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 5.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![3.0, 4.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();
    create_vector_collection(&cassie, "vector_search_normalized_fallback", 3);
    create_vector_index(&cassie, "vector_search_normalized_fallback", "cosine");
    let first_id = create_labelled_document(
        &cassie,
        "vector_search_normalized_fallback",
        "alpha",
        "first",
    );
    let second_id = create_labelled_document(
        &cassie,
        "vector_search_normalized_fallback",
        "beta",
        "second",
    );

    let before = cassie.metrics();

    // Act
    let normalized_search =
        vector_search(&cassie, "vector_search_normalized_fallback", "cosine", 2, 0);
    let after_normalized = cassie.metrics();

    clear_normalized_sidecars(&cassie, "vector_search_normalized_fallback", "embedding");

    let fallback_search =
        vector_search(&cassie, "vector_search_normalized_fallback", "cosine", 2, 0);
    let after_fallback = cassie.metrics();

    // Assert
    assert_eq!(normalized_search, fallback_search);
    assert_eq!(row_ids(&normalized_search), vec![first_id, second_id]);
    assert_normalized_fallback_metrics(&before, &after_normalized, &after_fallback);

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_create_hnsw_vector_index_with_rest_option_parity() {
    // Arrange
    let path = data_dir("rest_hnsw_options");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_hnsw_options", 3);

    // Act
    let created = create_vector_index_with_options(
        &cassie,
        "rest_hnsw_options",
        &serde_json::json!({
            "source_field": "content",
            "metric": "l2",
            "index_type": "hnsw",
            "m": "12",
            "ef_construction": "96",
            "ef_search": "48",
        }),
    );
    let stored = cassie
        .midge
        .get_vector_index(&canonical_collection("rest_hnsw_options"), "embedding")
        .unwrap()
        .expect("stored vector index");

    // Assert
    assert_eq!(created["index_type"], "hnsw");
    assert_eq!(stored.metadata.index_type.as_str(), "hnsw");
    let hnsw = stored.metadata.hnsw.expect("hnsw options");
    assert_eq!(hnsw.m, 12);
    assert_eq!(hnsw.ef_construction, 96);
    assert_eq!(hnsw.ef_search, 48);

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_search_rest_hnsw_vector_index_with_graph_execution() {
    // Arrange
    let path = data_dir("rest_hnsw_search");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 1.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
    ]);
    let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_hnsw_search", 3);
    create_vector_index_with_options(
        &cassie,
        "rest_hnsw_search",
        &serde_json::json!({
            "source_field": "content",
            "metric": "l2",
            "index_type": "hnsw",
            "m": "2",
            "ef_construction": "4",
            "ef_search": "2",
        }),
    );
    let nearest = create_labelled_document(&cassie, "rest_hnsw_search", "near", "first");
    let _ = create_labelled_document(&cassie, "rest_hnsw_search", "far", "second");
    let before = cassie.metrics();

    // Act
    let search = vector_search(&cassie, "rest_hnsw_search", "l2", 1, 0);
    let after = cassie.metrics();

    // Assert
    assert_eq!(row_ids(&search), vec![nearest]);
    assert_eq!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            - before["vector"]["hnsw_executions"].as_u64().unwrap(),
        1
    );

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_search_rest_ivfflat_vector_index_with_trained_candidates() {
    // Arrange
    let path = data_dir("rest_ivfflat_search");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![0.0, 1.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
    ]);
    let cassie = Cassie::new_in_memory_with_config(tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_ivfflat_search", 3);
    let created = create_vector_index_with_options(
        &cassie,
        "rest_ivfflat_search",
        &serde_json::json!({
            "source_field": "content",
            "metric": "l2",
            "index_type": "ivfflat",
            "lists": "2",
            "probes": "1",
            "training_sample_size": "2",
            "training_seed": "7",
        }),
    );
    let nearest = create_labelled_document(&cassie, "rest_ivfflat_search", "near", "first");
    let _ = create_labelled_document(&cassie, "rest_ivfflat_search", "far", "second");
    let before = cassie.metrics();

    // Act
    let search = vector_search(&cassie, "rest_ivfflat_search", "l2", 1, 0);
    let after = cassie.metrics();

    // Assert
    assert_eq!(created["index_type"], "ivfflat");
    assert_eq!(row_ids(&search), vec![nearest]);
    assert_eq!(
        after["vector"]["ivfflat_executions"].as_u64().unwrap()
            - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
        1
    );

    cleanup_path(Path::new(&path_for_cleanup));
}

#[test]
fn should_search_vector_docs_with_ollama_provider() {
    // Arrange
    let path = data_dir("ollama_search_flow");
    let path_for_cleanup = path.clone();

    let embedding_server = MockOpenAiServer::spawn(vec![
        MockResponse {
            status: 200,
            body: ollama_response_body(&[vec![1.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: ollama_response_body(&[vec![5.0, 0.0, 0.0]]),
        },
        MockResponse {
            status: 200,
            body: ollama_response_body(&[vec![0.0, 0.0, 0.0]]),
        },
    ]);

    let cassie = Cassie::new_in_memory_with_config(ollama_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

    // Act
    let rows = search_self_hosted_vector_docs(&cassie, "ollama_search_collection");
    // Assert
    assert_eq!(rows.len(), 2);

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_fail_vector_search_when_metric_incompatible_with_index() {
    // Arrange
    let path = data_dir("search_incompatible_metric");
    let path_for_cleanup = path.clone();

    let openai_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_in_memory_with_config(openai_runtime_with_server(openai_server.base_url()),
    )
    .unwrap();

    cassie.startup().unwrap();

    rest::collections::create(
        &cassie,
        serde_json::json!({
            "name": "search_incompatible_collection",
            "fields": [
                {"name": "content", "type": "text"},
                {"name": "embedding", "type": "vector(1536)"},
            ],
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    rest::indexes::create(
        &cassie,
        "search_incompatible_collection",
        serde_json::json!({
            "kind": "vector",
            "field": "embedding",
            "options": {
                "source_field": "content",
                "metric": "cosine",
            }
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    // Act
    let result = rest::search::vector_search(
        &cassie,
        "search_incompatible_collection",
        serde_json::json!({
            "field": "embedding",
            "query": "query text",
            "metric": "l2",
        })
        .to_string()
        .as_bytes(),
    );

    // Assert
    assert!(result.is_err());

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}
