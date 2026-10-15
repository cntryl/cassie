#[test]
fn should_assign_an_array_element_in_update() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_element_assignment (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_element_assignment VALUES (1, $1)",
            vec![Value::Json(serde_json::json!([10, 20, 30]))],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "UPDATE issue_807_array_element_assignment SET items[2] = 99 WHERE id = 1 RETURNING items::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-element-assignment", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{10,99,30}".to_owned())]]
    );
}

#[test]
fn should_expand_an_array_for_out_of_range_assignment() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_expansion (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_expansion VALUES (1, $1)",
            vec![Value::Json(serde_json::json!([10, 20]))],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "UPDATE issue_807_array_expansion SET items[5] = 99 WHERE id = 1 RETURNING items::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-indexed-update-expansion", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{10,20,NULL,NULL,99}".to_owned())]]
    );
}

#[test]
fn should_assign_an_array_slice_in_update() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_slice_assignment (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_slice_assignment VALUES (1, $1)",
            vec![Value::Json(serde_json::json!([10, 20, 30]))],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "UPDATE issue_807_array_slice_assignment SET items[2:3] = ARRAY[88,77] WHERE id = 1 RETURNING items::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-slice-assignment", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{10,88,77}".to_owned())]]
    );
}
