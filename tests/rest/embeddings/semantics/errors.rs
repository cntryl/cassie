#[test]
fn should_reject_invalid_rest_vector_index_options() {
    // Arrange
    use_local_storage();
    let path = data_dir("rest_invalid_hnsw_options");
    let path_for_cleanup = path.clone();
    let embedding_server = MockOpenAiServer::spawn(vec![]);
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        tei_runtime_with_server(embedding_server.base_url()),
    )
    .unwrap();
    cassie.startup().unwrap();
    create_vector_collection(&cassie, "rest_invalid_hnsw_options", 3);

    // Act
    let error = rest::indexes::create(
        &cassie,
        "rest_invalid_hnsw_options",
        serde_json::json!({
            "kind": "vector",
            "field": "embedding",
            "options": {
                "source_field": "content",
                "index_type": "hnsw",
                "m": "1",
            }
        })
        .to_string()
        .as_bytes(),
    )
    .expect_err("invalid hnsw m should fail");

    // Assert
    assert!(error
        .to_string()
        .contains("vector index option 'm' must be in [2, 128]"));

    cleanup_path(Path::new(&path_for_cleanup));
}
