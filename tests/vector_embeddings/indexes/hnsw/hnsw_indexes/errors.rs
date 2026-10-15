#[test]
fn should_reject_hnsw_sql_top_k_query_dimension_mismatch() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_sql_topk_dimension_mismatch");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_sql_topk_dimension_mismatch";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    let session = cassie.create_session("tester", None);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM hnsw_sql_topk_dimension_mismatch ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .expect_err("dimension mismatch should fail");

    // Assert
    let error = error.to_string();
    assert!(error.contains("vector distance query for field 'embedding'"));
    assert!(error.contains("expects 3 dimensions but received 2"));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_pgvector_operator_query_dimension_mismatch() {
    // Arrange
    use_local_storage();
    let path = data_dir("pgvector_operator_dimension_mismatch");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "pgvector_operator_dimension_mismatch";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    let empty_collection = "pgvector_operator_dimension_mismatch_empty";
    register_hnsw_collection(&cassie, empty_collection);
    let session = cassie.create_session("tester", None);

    // Act
    let errors = ["<->", "<=>", "<#>"].map(|operator| {
        cassie
            .execute_sql(
                &session,
                &format!("SELECT embedding {operator} '[1,0]' FROM {collection}"),
                vec![],
            )
            .expect_err("mismatched vector dimensions must be rejected")
            .to_string()
    });
    let empty_results = ["<->", "<=>", "<#>"].map(|operator| {
        cassie.execute_sql(
            &session,
            &format!("SELECT embedding {operator} '[1,0]' FROM {empty_collection}"),
            vec![],
        )
    });
    let empty_operand_results = [
        cassie.execute_sql(
            &session,
            &format!("SELECT embedding <-> '[]' FROM {collection}"),
            vec![],
        ),
        cassie.execute_sql(
            &session,
            &format!("SELECT embedding <-> $1 FROM {collection}"),
            vec![Value::String("[]".to_string())],
        ),
        cassie.execute_sql(
            &session,
            &format!("SELECT embedding <-> $1 FROM {collection}"),
            vec![Value::Vector(cassie::types::Vector::new(vec![]))],
        ),
    ];

    // Assert
    for error in errors {
        assert!(
            error.contains("vector distance"),
            "unexpected error: {error}"
        );
    }
    assert!(
        empty_results.iter().all(Result::is_err),
        "dimension mismatches must be rejected even when the table is empty"
    );
    assert!(
        empty_operand_results.iter().all(Result::is_err),
        "empty vector operands must be rejected rather than ranked as Infinity"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_out_of_range_query_vector_components() {
    // Arrange
    use_local_storage();
    let path = data_dir("out_of_range_query_vector_components");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "out_of_range_query_vector_components";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    let empty_collection = "out_of_range_query_vector_components_empty";
    register_hnsw_collection(&cassie, empty_collection);
    let session = cassie.create_session("tester", None);

    // Act
    let mut results = Vec::new();
    for query in ["[1e39,0,0]", "[-1e39,0,0]"] {
        results.push(cassie.execute_sql(
            &session,
            &format!("SELECT embedding <-> '{query}' FROM {collection}"),
            vec![],
        ));
        results.push(cassie.execute_sql(
            &session,
            &format!("SELECT embedding <-> $1 FROM {collection}"),
            vec![Value::String(query.to_string())],
        ));
        results.push(cassie.execute_sql(
                &session,
                &format!("SELECT vector_distance(embedding, '{query}') AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"),
                vec![],
            ));
        results.push(cassie.execute_sql(
                &session,
                &format!("SELECT vector_distance(embedding, $1) AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"),
                vec![Value::String(query.to_string())],
            ));
    }
    results.push(cassie.execute_sql(
            &session,
            &format!("SELECT vector_distance(embedding, $1) AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"),
            vec![Value::Json(serde_json::json!([1e39, 0.0, 0.0]))],
        ));
    results.push(cassie.execute_sql(
        &session,
        &format!("SELECT embedding <-> $1 FROM {collection}"),
        vec![Value::Vector(cassie::types::Vector::new(vec![
            1.0,
            f32::INFINITY,
            0.0,
        ]))],
    ));
    results.push(cassie.execute_sql(
            &session,
            &format!("SELECT vector_distance(embedding, $1) AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"),
            vec![Value::Vector(cassie::types::Vector::new(vec![
                1.0,
                f32::INFINITY,
                0.0,
            ]))],
        ));
    let empty_results = [
            cassie.execute_sql(
                &session,
                &format!("SELECT embedding <-> '[1e39,0,0]' FROM {empty_collection}"),
                vec![],
            ),
            cassie.execute_sql(
                &session,
                &format!("SELECT vector_distance(embedding, '[1e39,0,0]') AS distance FROM {empty_collection} ORDER BY distance ASC LIMIT 1"),
                vec![],
            ),
        ];

    // Assert
    assert!(
        results.iter().all(Result::is_err),
        "out-of-range query vectors must be rejected in every supported form"
    );
    assert!(
        empty_results.iter().all(Result::is_err),
        "out-of-range literals must be rejected even when the table is empty"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_hnsw_graph_entry_point_is_invalid() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_invalid_entry_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_invalid_entry_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.entry_point = Some("missing-entry".to_string());
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "missing-entry-point";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_hnsw_graph_max_layer_is_invalid() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_invalid_max_layer_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_invalid_max_layer_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.max_layer = graph.max_layer.saturating_add(1);
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "inconsistent-max-layer";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}
