#[test]
fn should_use_hnsw_graph_for_sql_vector_top_k_with_bounded_candidates() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_sql_topk");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_sql_topk";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "middle", [0.6, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_sql_topk ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            - before["vector"]["hnsw_executions"].as_u64().unwrap(),
        1
    );
    assert!(
        after["vector"]["candidate_count_total"].as_u64().unwrap()
            - before["vector"]["candidate_count_total"].as_u64().unwrap()
            < 4
    );

    let _ = std::fs::remove_dir_all(path);
}
