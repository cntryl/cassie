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

#[test]
fn should_coerce_array_literals_for_slice_assignment() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_1460_array_slice_literal (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_1460_array_slice_literal VALUES (1, '{0,0}')",
            Vec::new(),
        ),
    ];
    let literal = fixture::execute_cycle(
        "UPDATE issue_1460_array_slice_literal SET items[1:2] = '{1,2}' RETURNING items::TEXT",
        None,
        0,
        true,
    );
    let constructor = fixture::execute_cycle(
        "UPDATE issue_1460_array_slice_literal SET items[1:2] = ARRAY[3,4] RETURNING items::TEXT",
        None,
        0,
        true,
    );
    let element = fixture::execute_cycle(
        "UPDATE issue_1460_array_slice_literal SET items[1] = '7' RETURNING items::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-1460-array-slice-literal",
        &setup,
        vec![literal, constructor, element],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{1,2}".to_owned())]]
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("{3,4}".to_owned())]]
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("{7,4}".to_owned())]]
    );
}

#[test]
fn should_report_array_literal_errors_for_slice_assignment() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_1460_array_slice_errors (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_1460_array_slice_errors VALUES (1, '{0,0}')",
            Vec::new(),
        ),
    ];
    let invalid_element = fixture::execute_cycle(
        "UPDATE issue_1460_array_slice_errors SET items[1:2] = '{x,y}'",
        None,
        0,
        true,
    );
    let mismatched_length = fixture::execute_cycle(
        "UPDATE issue_1460_array_slice_errors SET items[1:2] = '{1}'",
        None,
        0,
        true,
    );
    let scalar = fixture::execute_cycle(
        "UPDATE issue_1460_array_slice_errors SET items[1:2] = '1'",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-1460-array-slice-literal-errors",
        &setup,
        vec![invalid_element, mismatched_length, scalar],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22000"));
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22P02"));
}
