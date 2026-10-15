#[test]
fn should_concatenate_integer_arrays_with_postgresql_result_identity() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT ARRAY[1, 2] || ARRAY[3] AS combined", None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-concatenation-result-identity", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{1,2,3}".to_owned())]]
    );
}

#[test]
fn should_filter_table_arrays_by_containment() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_operator_rows (id INT, items INT[], required INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_operator_rows VALUES (1, $1, $2), (2, $3, $4), (3, $5, $6)",
            vec![
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([2])),
                Value::Json(serde_json::json!([4, 5])),
                Value::Json(serde_json::json!([2])),
                Value::Json(serde_json::json!([2, 3])),
                Value::Json(serde_json::json!([3])),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT id FROM issue_807_array_operator_rows WHERE items @> required ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-containment-predicate", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned())], vec![Some("3".to_owned())]]
    );
}

#[test]
fn should_filter_table_arrays_by_overlap() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_overlap_rows (id INT, items INT[], probe INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_overlap_rows VALUES (1, $1, $2), (2, $3, $4), (3, $5, $6)",
            vec![
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([2, 3])),
                Value::Json(serde_json::json!([4, 5])),
                Value::Json(serde_json::json!([2, 3])),
                Value::Json(serde_json::json!([2, 3])),
                Value::Json(serde_json::json!([3])),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT id FROM issue_807_array_overlap_rows WHERE items && probe ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-overlap-predicate", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned())], vec![Some("3".to_owned())]]
    );
}
