#[test]
fn should_exclude_null_vector_rows_from_sql_top_k() {
    // Arrange
    let path = data_dir("sql_topk_missing_vector");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let collection = "sql_topk_missing_vector";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [0.0, 1.0, 0.0]);
    cassie
        .midge
        .put_document(
            &canonical_hnsw_collection(collection),
            Some("missing".to_string()),
            serde_json::json!({"content": "missing vector"}),
        )
        .expect("insert row with NULL vector");
    let session = cassie.create_session("tester", None);
    let query = format!(
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {collection} ORDER BY distance ASC LIMIT 2"
        );

    // Act
    let exact = cassie.execute_sql(&session, &query, vec![]);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &session,
            &format!("UPDATE {collection} SET content = 'near updated' WHERE id = 'near'"),
            vec![],
        )
        .expect("stage an update to select the non-streaming exact path");
    let fallback = cassie.execute_sql(&session, &query, vec![]);
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback transaction");
    put_hnsw_index(&cassie, collection, 2);
    let indexed = cassie.execute_sql(&session, &query, vec![]);

    // Assert
    let expected = vec![
        Value::String("near".to_string()),
        Value::String("far".to_string()),
    ];
    let exact = exact.expect("exact top-k should skip NULL vectors");
    let exact_rows = exact.rows();
    let fallback = fallback.expect("non-streaming exact top-k should skip NULL vectors");
    let fallback_rows = fallback.rows();
    let indexed = indexed.expect("HNSW top-k should skip NULL vectors");
    let indexed_rows = indexed.rows();
    assert_eq!(exact_rows.len(), expected.len());
    assert_eq!(fallback_rows.len(), expected.len());
    assert_eq!(indexed_rows.len(), expected.len());
    assert_eq!(
        exact_rows
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        indexed_rows
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        fallback_rows
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>(),
        expected
    );
    let _ = std::fs::remove_dir_all(path);
}
