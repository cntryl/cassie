#[test]
fn should_use_array_btree_range_predicates_on_real_rows() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_range_index (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_array_range_index_items_idx ON issue_807_array_range_index USING btree (items)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_range_index VALUES (1, $1), (2, $2), (3, $3), (4, $4), (5, $5), (6, $6), (7, NULL)",
            vec![
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([1, 2, 0])),
                Value::Json(serde_json::json!([2, 0])),
                Value::Json(serde_json::json!([1, 3])),
                Value::Json(serde_json::json!([0, 9])),
                Value::Json(serde_json::json!([[1, 2], [3, 4]])),
            ],
        ),
    ];
    let query = fixture::execute_cycle(
        "SELECT id FROM issue_807_array_range_index WHERE items < $1 ORDER BY id",
        Some((1007, 0, Some(b"{2,0}"))),
        0,
        true,
    );
    let explain = fixture::execute_cycle(
        "EXPLAIN SELECT id FROM issue_807_array_range_index WHERE items < ARRAY[2,0]",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-array-btree-range-predicate",
        &setup,
        vec![query, explain],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("2".to_owned())],
            vec![Some("4".to_owned())],
            vec![Some("5".to_owned())],
            vec![Some("6".to_owned())],
        ]
    );

    let explain = &batches[1];
    fixture::assert_success(explain);
    let explain_rows = wire::data_rows(explain);
    let plan = explain_rows
        .first()
        .and_then(|row| row.first())
        .and_then(Option::as_deref)
        .expect("array index range plan");
    assert!(plan.contains("index=issue_807_array_range_index_items_idx"));
    assert!(plan.contains("access_path=range_scan"));
}
