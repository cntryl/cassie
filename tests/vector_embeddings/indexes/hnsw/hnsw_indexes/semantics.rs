#[test]
fn should_clean_batched_hnsw_sidecars_after_failed_publication_retry() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    use_local_storage();
    let path = data_dir("hnsw_batched_build_retry");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let collection = "hnsw_batched_build_retry";
    register_hnsw_collection(&cassie, collection);
    let canonical_collection = canonical_hnsw_collection(collection);
    let documents = (0..5_001)
        .map(|index| {
            let ordinate = f64::from(u32::try_from(index % 97).expect("small ordinate")) + 1.0;
            (
                Some(format!("document-{index:05}")),
                serde_json::json!({
                    "content": format!("document-{index:05}"),
                    "embedding": [1.0, ordinate, 0.5]
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_documents(&canonical_collection, documents)
        .expect("seed documents");
    let record = hnsw_index_record(collection, 2);
    let normalized_prefix = cassie
        .midge
        .normalized_vector_prefix_for_diagnostics(&canonical_collection, "embedding")
        .expect("normalized-vector prefix");
    let node_prefix = cassie
        .midge
        .hnsw_node_prefix_for_diagnostics(&canonical_collection, "embedding")
        .expect("node prefix");

    // Act
    set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorState));
    let failed = cassie.midge.put_vector_index(record.clone());
    let unpublished = cassie
        .midge
        .get_vector_index(&canonical_collection, "embedding")
        .expect("read unpublished index");
    let staged_nodes = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &node_prefix)
        .expect("scan staged nodes");
    set_document_write_failure_point(None);
    cassie
        .midge
        .put_vector_index(record)
        .expect("retry index build");
    let published = stored_hnsw_index(&cassie, collection);
    cassie
        .midge
        .delete_vector_index(&canonical_collection, "embedding")
        .expect("delete batched vector sidecars");
    let remaining_normalized = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &normalized_prefix)
        .expect("scan remaining normalized vectors");
    let remaining_nodes = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &node_prefix)
        .expect("scan remaining HNSW nodes");

    // Assert
    let failed = failed.expect_err("manifest publication should fail");
    assert!(failed.to_string().contains("injected test failure"));
    assert!(unpublished.is_none());
    assert_eq!(staged_nodes.len(), 5_001);
    assert_eq!(
        published
            .metadata
            .hnsw_graph
            .expect("published graph")
            .row_count,
        5_001
    );
    assert_eq!(
        remaining_normalized,
        [] as [(std::vec::Vec<u8>, std::vec::Vec<u8>); 0]
    );
    assert_eq!(
        remaining_nodes,
        [] as [(std::vec::Vec<u8>, std::vec::Vec<u8>); 0]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_hnsw_graph_state_in_the_data_family() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_graph_data_family");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let collection = "hnsw_graph_data_family";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let raw_metadata = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Schema, b"")
        .expect("schema scan")
        .into_iter()
        .find_map(|(_key, value)| serde_json::from_slice::<VectorIndexRecord>(&value).ok())
        .expect("vector index metadata");
    let state = cassie
        .midge
        .get_vector_index_state(&canonical_hnsw_collection(collection), "embedding")
        .expect("read state")
        .expect("persisted state");

    // Assert
    assert!(raw_metadata.metadata.hnsw_graph.is_none());
    assert!(state.hnsw_graph.is_some());
    assert!(state.ivfflat_training.is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_refresh_hnsw_graph_after_document_mutations() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_graph_refresh");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_graph_refresh";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    assert_eq!(
        stored_hnsw_index(&cassie, collection)
            .metadata
            .hnsw_graph
            .unwrap()
            .row_count,
        2
    );

    // Act
    put_hnsw_document(&cassie, collection, "new-nearest", [0.9, 0.0, 0.0]);
    let after_insert = stored_hnsw_index(&cassie, collection);
    cassie
        .midge
        .delete_document(&canonical_hnsw_collection(collection), "far")
        .expect("delete document");
    let after_delete = stored_hnsw_index(&cassie, collection);

    // Assert
    assert_eq!(after_insert.metadata.hnsw_graph.unwrap().row_count, 3);
    assert_eq!(after_delete.metadata.hnsw_graph.unwrap().row_count, 2);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_hnsw_reads_safe_during_concurrent_mutation() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_concurrent_mutation");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_concurrent_mutation";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    let cassie = std::sync::Arc::new(cassie);

    // Act
    let readers = (0..4)
        .map(|_| {
            let cassie = std::sync::Arc::clone(&cassie);
            std::thread::spawn(move || {
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_concurrent_mutation ORDER BY distance ASC LIMIT 1",
                        vec![],
                    )
                    .unwrap()
                    .rows()[0][0]
                    .clone()
            })
        })
        .collect::<Vec<_>>();
    let writer = {
        let cassie = std::sync::Arc::clone(&cassie);
        std::thread::spawn(move || {
            put_hnsw_document(
                cassie.as_ref(),
                "hnsw_concurrent_mutation",
                "new-nearest",
                [0.99, 0.0, 0.0],
            );
        })
    };
    writer.join().unwrap();
    let results = readers
        .into_iter()
        .map(|reader| reader.join().unwrap())
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(results.len(), 4);
    assert!(results.into_iter().all(|value| {
        value == Value::String("near".to_string())
            || value == Value::String("new-nearest".to_string())
    }));
    assert_eq!(
        stored_hnsw_index(cassie.as_ref(), collection)
            .metadata
            .hnsw_graph
            .unwrap()
            .row_count,
        4
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_not_use_a_vector_index_on_a_case_distinct_sibling_field() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_case_distinct_vector_fields");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = "hnsw_case_distinct_vector_fields";
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "CREATE TABLE hnsw_case_distinct_vector_fields (content TEXT, \
                 \"a\" VECTOR(3), \"A\" VECTOR(3))",
            vec![],
        )
        .expect("create case-distinct vector table");
    let canonical_collection = canonical_hnsw_collection(collection);
    for (id, lower, upper) in [
        ("lower-near", [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ("upper-near", [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
    ] {
        cassie
            .midge
            .put_document(
                &canonical_collection,
                Some(id.to_string()),
                serde_json::json!({
                    "content": id,
                    "a": lower,
                    "A": upper
                }),
            )
            .expect("seed case-distinct vector document");
    }
    let mut index = hnsw_index_record(collection, 2);
    index.field = "a".to_string();
    index.source_field = "content".to_string();
    index.metadata.dimensions = 3;
    cassie
        .midge
        .put_vector_index(index.clone())
        .expect("publish lower-field HNSW index");
    cassie.register_vector_index(index);
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();

    // Act
    let lower = cassie
        .execute_sql(
            &session,
            "SELECT _id, vector_distance(\"a\", '[0,0,0]') AS distance \
                 FROM hnsw_case_distinct_vector_fields ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .expect("query indexed lower-case vector field");
    let after_lower = cassie.metrics();
    let upper = cassie
        .execute_sql(
            &session,
            "SELECT _id, vector_distance(\"A\", '[0,0,0]') AS distance \
                 FROM hnsw_case_distinct_vector_fields ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .expect("query exact case-distinct vector field");
    let after_upper = cassie.metrics();

    // Assert
    assert_eq!(lower.rows()[0][0], Value::String("lower-near".to_string()));
    assert_eq!(
        after_lower["vector"]["hnsw_executions"].as_u64().unwrap()
            - before["vector"]["hnsw_executions"].as_u64().unwrap(),
        1
    );
    assert_eq!(upper.rows()[0][0], Value::String("upper-near".to_string()));
    assert_eq!(
        after_upper["vector"]["hnsw_executions"].as_u64(),
        after_lower["vector"]["hnsw_executions"].as_u64()
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_hnsw_graph_for_an_other_case_vector_field() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_sql_topk_field_case");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_sql_topk_field_case";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
            &session,
            "SELECT id, vector_distance(EMBEDDING, '[1,0,0]') AS distance FROM hnsw_sql_topk_field_case ORDER BY distance ASC LIMIT 1",
            vec![],
        );

    // Assert
    let rows = result
        .as_ref()
        .map(cassie::executor::QueryResult::rows)
        .ok();
    assert_eq!(
        rows.and_then(|rows| rows.first().map(|row| row[0].clone())),
        Some(Value::String("near".to_string()))
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_deterministically_when_hnsw_graph_is_missing() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_missing_graph_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_missing_graph_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    clear_stored_hnsw_graph(&cassie, collection);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_missing_graph_fallback ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
            - before["vector"]["hnsw_fallbacks"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        after["vector"]["last_fallback_reason"].as_str(),
        Some("missing-graph")
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_same_row_count_hnsw_graph_fingerprint_is_stale() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_stale_fingerprint_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_stale_fingerprint_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.source_fingerprint ^= 1;
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "stale-source-fingerprint";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_hnsw_graph_neighbor_reference_is_corrupt() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_corrupt_neighbor_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_corrupt_neighbor_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.nodes[0].layers[0].push("missing-neighbor".to_string());
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "unknown-neighbor-id";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_omit_names_from_hnsw_hot_keys() {
    // Arrange
    let (cassie, path, collection) = hnsw_layout_fixture("hnsw_numeric_keys");

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let (prefix, _, _, _, _) = inspect_hnsw_layout(&cassie, collection);

    // Assert
    assert!(!prefix
        .windows(collection.len())
        .any(|window| window == collection.as_bytes()));
    assert!(!prefix
        .windows("embedding".len())
        .any(|window| window == b"embedding"));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_scale_hnsw_query_reads_with_reachable_nodes_not_corpus_size() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_point_read_scaling");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_point_read_scaling";
    register_hnsw_collection(&cassie, collection);
    for index in 0..32 {
        let value = if index == 0 { 1.0 } else { -1.0 };
        put_hnsw_document(
            &cassie,
            collection,
            &format!("doc-{index}"),
            [value, 0.0, 0.0],
        );
    }
    put_hnsw_index(&cassie, collection, 2);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_point_read_scaling ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(reads < 32, "expected bounded point reads, observed {reads}");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_remove_hnsw_point_nodes_when_index_is_dropped() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_point_nodes_drop_cleanup");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_point_nodes_drop_cleanup";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);

    // Act
    cassie
        .midge
        .delete_vector_index(&canonical_hnsw_collection(collection), "embedding")
        .unwrap();
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();

    // Assert
    assert!(!entries.iter().any(|(_, raw)| {
        serde_json::from_slice::<cassie::embeddings::HnswGraphNode>(raw).is_ok()
    }));

    let _ = std::fs::remove_dir_all(path);
}
