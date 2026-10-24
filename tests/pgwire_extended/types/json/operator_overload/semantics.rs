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
fn should_delete_jsonb_array_elements_by_integer_index() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_integer (array_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_integer VALUES ('[0,1,2,3]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT array_document - 1, array_document - (-1), array_document - 1::SMALLINT, \
         array_document - 9 FROM issue_809_jsonb_delete_integer",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-integer", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("integer deletion RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114; 4]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0, 2, 3]".to_owned()),
            Some("[0, 1, 2]".to_owned()),
            Some("[0, 2, 3]".to_owned()),
            Some("[0, 1, 2, 3]".to_owned()),
        ]]
    );
}

#[test]
fn should_remove_multiple_jsonb_object_keys_from_a_text_array() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"a\":1,\"b\":2,\"c\":3}'::JSONB - ARRAY['a','c']",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-text-array-object", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text-array deletion RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"b\": 2}".to_owned())]]
    );
}

#[test]
fn should_remove_matching_jsonb_array_strings_using_text_array_keys() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '[\"a\",\"b\",\"a\",2]'::JSONB - ARRAY['a','missing'], \
         '[\"a\",\"b\",\"a\",2]'::JSONB - ARRAY[NULL::TEXT,'b']",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-text-array", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array text-array deletion RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114; 2]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[\"b\", 2]".to_owned()),
            Some("[\"a\", \"a\", 2]".to_owned()),
        ]]
    );
}

#[test]
fn should_delete_nested_jsonb_values_with_the_hash_minus_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_path (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_path VALUES ('{\"a\":{\"b\":1,\"c\":2},\"z\":3}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #- ARRAY['a','b'] FROM issue_809_jsonb_delete_path",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("path deletion RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 114);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": {\"c\": 2}, \"z\": 3}".to_owned())]]
    );
}

#[test]
fn should_delete_a_nested_jsonb_array_object_path() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"items\":[{\"drop\":0,\"keep\":{\"n\":9007199254740993,\"scale\":1.00,\"exponent\":1e2}}]}'::JSONB \
         #- ARRAY['items','0','drop'])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-mixed-depth", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(
            "{\"items\": [{\"keep\": {\"n\": 9007199254740993, \"scale\": 1.00, \"exponent\": 100}}]}"
                .to_owned()
        )]]
    );
}

#[test]
fn should_keep_the_last_duplicate_jsonb_key_after_deletion() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"a\":1,\"a\":2,\"drop\":0}'::JSONB - 'drop')::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-duplicate-key", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"a\": 2}".to_owned())]]
    );
}

#[test]
fn should_delete_a_jsonb_value_at_a_negative_array_path_index() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"items\":[1,2,3]}'::JSONB #- ARRAY['items','-1'])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-negative-index", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"items\": [1, 2]}".to_owned())]]
    );
}

#[test]
fn should_leave_jsonb_unchanged_for_a_valid_out_of_bounds_path_index() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('[0,1,2]'::JSONB #- ARRAY['9'])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-out-of-bounds", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[0, 1, 2]".to_owned())]]
    );
}

#[test]
fn should_leave_jsonb_unchanged_for_a_missing_path_segment() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"items\":[1,2,3]}'::JSONB #- ARRAY['missing','not-an-index'])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-missing", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"items\": [1, 2, 3]}".to_owned())]]
    );
}

#[test]
fn should_leave_jsonb_unchanged_when_a_deletion_path_enters_a_scalar() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"value\":1}'::JSONB #- ARRAY['value','next'])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-scalar-child", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"value\": 1}".to_owned())]]
    );
}

#[test]
fn should_leave_jsonb_unchanged_for_an_empty_deletion_path() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"items\":[1,2,3]}'::JSONB #- ARRAY[]::TEXT[])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-empty", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"items\": [1, 2, 3]}".to_owned())]]
    );
}

#[test]
fn should_infer_text_array_for_a_jsonb_path_deletion_parameter() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_path_parameter (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_path_parameter VALUES ('{\"a\":{\"b\":1,\"c\":2},\"z\":3}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT (document #- $1)::TEXT FROM issue_809_jsonb_delete_path_parameter",
        Some((0, 0, Some(b"{a,b}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-delete-path-parameter",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let parameter_types = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("path parameter description");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [1009]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": {\"c\": 2}, \"z\": 3}".to_owned())]]
    );
}

#[test]
fn should_propagate_sql_null_through_jsonb_deletion_overloads() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('[0,1]'::JSONB - NULL::INTEGER)::TEXT, \
         ('{}'::JSONB - NULL::TEXT[])::TEXT, \
         ('{}'::JSONB #- NULL::TEXT[])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-null", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None, None, None]]);
}

#[test]
fn should_reject_jsonb_integer_deletion_from_objects() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT '{}'::JSONB - 1", None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-integer-object", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22023"));
}

#[test]
fn should_report_invalid_jsonb_array_deletion_paths() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('[0,1]'::JSONB #- ARRAY['not-an-index']::TEXT[])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-invalid-path", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}

#[test]
fn should_reject_jsonb_array_path_indices_outside_integer_range() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '[0,1,2]'::JSONB #- ARRAY['2147483648']",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-index-range", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}

#[test]
fn should_parse_integer_syntax_for_jsonb_array_path_indices() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('[0,1,2]'::JSONB #- ARRAY['+1'])::TEXT, \
         ('[0,1,2]'::JSONB #- ARRAY[' 1 '])::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-index-syntax", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[0, 2]".to_owned()), Some("[0, 2]".to_owned())]]
    );
}

#[test]
fn should_reject_jsonb_path_deletion_from_scalar_documents() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '42'::JSONB #- ARRAY['value']",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-scalar", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22023"));
}

#[test]
fn should_reject_null_elements_in_jsonb_deletion_paths() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"a\":1}'::JSONB #- ARRAY[NULL::TEXT]",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-path-null-item", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22004"));
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
