#[test]
fn should_count_elements_in_a_jsonb_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_length_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_length_values VALUES ('[2,4]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_array_length(document) FROM issue_810_jsonb_length_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-array-length", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_detect_any_jsonb_key_from_a_text_array_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_exists_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_exists_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_exists_any(document, '{a,b}') FROM issue_810_jsonb_exists_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-exists-array-functions",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_require_all_jsonb_keys_from_a_text_array_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_exists_all_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_exists_all_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_exists_all(document, '{a,b}') FROM issue_810_jsonb_exists_all_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-exists-all-array-function",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("f".to_owned())]]);
}

#[test]
fn should_return_jsonb_text_with_the_arrow_text_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_arrow_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_arrow_text_values VALUES ('{\"label\":\"value\"}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document ->> 'label' FROM issue_809_arrow_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-arrow-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("value".to_owned())]]
    );
}

#[test]
fn should_extract_nested_jsonb_values_with_the_path_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #> '{nested,x}' FROM issue_809_path_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_extract_nested_jsonb_scalars_as_text_with_the_path_text_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_text_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #>> '{nested,x}' FROM issue_809_path_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

