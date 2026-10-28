#[test]
fn should_return_the_jsonb_type_name_for_a_stored_document() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_typeof_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_typeof_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_typeof(document) FROM issue_810_typeof_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-typeof", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("object".to_owned())]]
    );
}

#[test]
fn should_pretty_print_jsonb_objects_with_nested_arrays() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_pretty_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_pretty_values VALUES ('{\"a\":1,\"b\":[2,3]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_pretty(document) FROM issue_810_pretty_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-pretty", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(
            "{\n    \"a\": 1,\n    \"b\": [\n        2,\n        3\n    ]\n}".to_owned()
        )]]
    );
}

#[test]
fn should_apply_the_jsonb_path_exists_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_exists_operator_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_exists_operator_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @? '$.a' FROM issue_809_path_exists_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-exists-operator", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_apply_the_jsonb_path_match_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_match_operator_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_match_operator_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @@ '$.a == 1' FROM issue_809_path_match_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-match-operator", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_extract_jsonb_path_text_with_the_path_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_extract_path_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_extract_path_text_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_extract_path_text(document, 'nested', 'x') FROM issue_810_extract_path_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-extract-path-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_replace_a_jsonb_value_at_a_path_with_jsonb_set() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_set_values (document JSONB, path TEXT[], replacement JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_set_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!({"a": 1, "b": 1})),
                Value::Json(serde_json::json!(["a"])),
                Value::Json(serde_json::json!(2)),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set(document, path, replacement)::TEXT FROM issue_810_jsonb_set_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 2, \"b\": 1}".to_owned())]]
    );
}

#[test]
fn should_apply_jsonpath_predicate_operators_with_boolean_wire_types() {
    // Arrange
    let sql = "SELECT payload @? '$.items[*] ? (@.qty >= 2)'::JSONPATH, payload @@ '$.active == true'::JSONPATH FROM (VALUES ('{\"active\":true,\"items\":[{\"qty\":1},{\"qty\":2}]}'::JSONB)) AS input(payload)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonpath-predicate-operators", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath predicate operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("t".to_owned())]]
    );
}
