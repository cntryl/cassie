#[test]
fn should_preserve_declared_json_recordset_fields() {
    // Arrange
    let sql = "SELECT id, n FROM jsonb_to_recordset('[{\"id\":1,\"n\":1},{\"id\":2,\"n\":null},{\"id\":3}]'::jsonb) AS records(id INTEGER, n INTEGER) ORDER BY id";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("type-json-recordset-null-fields", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("recordset RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), None],
            vec![Some("3".to_owned()), None],
        ]
    );
}

#[test]
fn should_convert_json_objects_to_declared_record_fields() {
    // Arrange
    let sql = "SELECT id, name FROM json_to_record('{\"id\":7,\"name\":\"Ada\"}') AS record(id INTEGER, name TEXT)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-to-record", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("json_to_record RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("7".to_owned()), Some("Ada".to_owned())]]
    );
}

#[test]
fn should_preserve_jsonb_values_when_expanding_array_elements() {
    // Arrange
    let sql = "SELECT jsonb_array_elements('[1,null,\"x\"]'::jsonb)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-array-elements", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB array-element RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3_802]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("null".to_owned())],
            vec![Some("\"x\"".to_owned())],
        ]
    );
}

#[test]
fn should_expand_jsonb_object_fields_in_select_list() {
    // Arrange
    let sql = "SELECT jsonb_each_text('{\"b\":2,\"a\":null}'::jsonb)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each-text", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (2_249, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            fixture::one_field_row(b"(a,)"),
            fixture::one_field_row(b"(b,2)"),
        ]
    );
}

#[test]
fn should_expand_jsonb_values_as_select_list_records() {
    // Arrange
    let sql = "SELECT jsonb_each('{\"b\":2,\"a\":null}'::jsonb)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (2_249, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            fixture::one_field_row(b"(a,null)"),
            fixture::one_field_row(b"(b,2)"),
        ]
    );
}

#[test]
fn should_expand_json_table_rows() {
    // Arrange
    let sql = r#"SELECT id FROM JSON_TABLE('[{"id":7},{"id":9}]', '$[*]' COLUMNS (id INTEGER PATH '$.id')) AS j ORDER BY id"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-rows", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"7"), fixture::one_field_row(b"9")]
    );
}

#[test]
fn should_expand_nested_json_table_paths_with_ordinality() {
    // Arrange
    let sql = r#"SELECT * FROM JSON_TABLE('[{"id":7,"items":[{"n":1},{"n":2}]},{"id":9,"items":[]}]', '$[*]' COLUMNS (id INTEGER PATH '$.id', NESTED PATH '$.items[*]' COLUMNS (ordinal FOR ORDINALITY, n INTEGER PATH '$.n' DEFAULT 0 ON EMPTY))) AS j ORDER BY id, ordinal"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-nested-ordinality", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("nested JSON_TABLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("1".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("7".to_owned()),
                Some("2".to_owned()),
                Some("2".to_owned()),
            ],
            vec![Some("9".to_owned()), None, None],
        ]
    );
}

#[test]
fn should_expand_jsonb_object_keys_as_text_rows() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_keys_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_keys_values VALUES ('{\"b\":2,\"a\":1}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_object_keys(document) FROM issue_810_jsonb_keys_values ORDER BY 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-object-keys", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("a".to_owned())], vec![Some("b".to_owned())]]
    );
}
