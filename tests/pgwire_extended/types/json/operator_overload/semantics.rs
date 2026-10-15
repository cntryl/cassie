#[test]
fn should_extract_json_documents_through_arrow_path_overloads() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_json_operator_values (document JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_json_operator_values VALUES ('{\"nested\":{\"x\":7,\"label\":\"Ada\"},\"items\":[4,5]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document -> 'nested', document ->> 'nested', \
         document #> '{nested,x}', document #>> '{nested,x}', \
         document -> 'items' -> 1, document -> 'items' ->> -1, \
         document #> '{items,1}', document #>> '{items,-1}' \
         FROM issue_809_json_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-json-arrow-path-overloads", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON extraction RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114, 25, 114, 25, 114, 25, 114, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"x":7,"label":"Ada"}"#.to_owned()),
            Some(r#"{"x":7,"label":"Ada"}"#.to_owned()),
            Some("7".to_owned()),
            Some("7".to_owned()),
            Some("5".to_owned()),
            Some("5".to_owned()),
            Some("5".to_owned()),
            Some("5".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_jsonb_object_equality_ordering() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_object_order (case_id INTEGER, left_document JSONB, right_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_object_order VALUES \
             (1, '{\"a\":1,\"b\":2}', '{\"b\":2,\"a\":1}'), \
             (2, '{\"a\":1}', '{\"b\":1}'), \
             (3, '{\"a\":1}', '{\"a\":2}'), (4, '[]', '{}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT case_id, left_document = right_document, left_document < right_document \
         FROM issue_809_jsonb_object_order ORDER BY case_id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-object-order", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB object comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
            vec![
                Some("3".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
            vec![
                Some("4".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
        ]
    );
}

#[test]
fn should_apply_jsonb_deletion_operator_overloads() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_overloads (array_document JSONB, object_document JSONB, nested_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_overloads VALUES ('[0,1,2,3]', '{\"a\":1,\"b\":2,\"c\":3}', '{\"a\":{\"b\":1,\"c\":2},\"z\":3}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT (array_document - 1)::TEXT, (array_document - (-1))::TEXT, \
         (object_document - ARRAY['a','c'])::TEXT, \
         (nested_document #- ARRAY['a','b'])::TEXT \
         FROM issue_809_jsonb_delete_overloads",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-overloads", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB deletion overload RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(25, -1, -1); 4]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0, 2, 3]".to_owned()),
            Some("[0, 1, 2]".to_owned()),
            Some("{\"b\": 2}".to_owned()),
            Some("{\"a\": {\"c\": 2}, \"z\": 3}".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_jsonb_array_containment_without_order_sensitivity() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_array_containment (case_id INTEGER, document JSONB, subset JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_array_containment VALUES \
             (1, '[1,2,3]', '[3,1]'), \
             (2, '{\"a\":[1,2,3]}', '{\"a\":[3,1]}'), \
             (3, '{\"a\":[1,2,3]}', '{\"a\":[4]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT case_id, document @> subset \
         FROM issue_809_jsonb_array_containment ORDER BY case_id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-array-containment", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB array containment RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("t".to_owned())],
            vec![Some("2".to_owned()), Some("t".to_owned())],
            vec![Some("3".to_owned()), Some("f".to_owned())],
        ]
    );
}

#[test]
fn should_concatenate_jsonb_array_scalar_shapes() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_concat_shapes (case_id INTEGER, left_document JSONB, right_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_concat_shapes VALUES \
             (1, '[1,2]', '[3]'), (2, '[1,2]', '3'), \
             (3, '1', '[2,3]'), (4, '{\"a\":1}', '[2]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT (left_document || right_document)::TEXT \
         FROM issue_809_jsonb_concat_shapes ORDER BY case_id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concat-shapes", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("[1, 2, 3]".to_owned())],
            vec![Some("[1, 2, 3]".to_owned())],
            vec![Some("[1, 2, 3]".to_owned())],
            vec![Some("[{\"a\": 1}, 2]".to_owned())],
        ]
    );
}
