#[test]
fn should_propagate_sql_null_through_typed_json_query_functions() {
    // Arrange
    let sql =
        "SELECT JSON_VALUE(NULL::JSONB, '$.n' RETURNING BIGINT ERROR ON EMPTY ERROR ON ERROR), \
                      JSON_QUERY(NULL::JSONB, '$.n' RETURNING JSONB ERROR ON EMPTY ERROR ON ERROR)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-null-typed-results", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("typed SQL/JSON RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(20, 8, -1), (3_802, -1, -1)]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![None, None]]);
}

#[test]
fn should_delete_a_jsonb_key_when_set_lax_receives_sql_null() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_set_lax_values (document JSONB, path TEXT[], replacement JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_set_lax_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!({"a": 1})),
                Value::Json(serde_json::json!(["a"])),
                Value::Null,
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set_lax(document, path, replacement, TRUE, 'delete_key') FROM issue_810_jsonb_set_lax_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-lax-delete", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"{}")]
    );
}

#[test]
fn should_order_json_array_aggregate_without_null_inputs() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_array_aggregate_order_input (n INT, value TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_array_aggregate_order_input VALUES (2, 'b'), (1, 'a'), (3, NULL)",
            Vec::new(),
        ),
    ];
    let sql = "SELECT JSON_ARRAYAGG(value ORDER BY n ABSENT ON NULL) FROM json_array_aggregate_order_input";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "type-json-array-aggregate-order-null-policy",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_ARRAYAGG RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 114);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[\"a\", \"b\"]".to_owned())]]
    );
}

#[test]
fn should_distinguish_json_null_from_missing_or_sql_null_with_jsonb_operators() {
    // Arrange
    let sql = "SELECT ('{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB -> 'present'), ('{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB -> 'missing'), '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB ->> 'present', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB #> '{nested,x}', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB #>> '{nested,x}', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB ? 'present', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB ? 'missing'";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-jsonb-operator-null-missing", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802, 25, 3802, 25, 16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("null".to_owned()),
            None,
            None,
            Some("true".to_owned()),
            Some("true".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_order_jsonb_aggregate_with_both_json_sql_null_inputs() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE jsonb_aggregate_order_input (ordinal INT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO jsonb_aggregate_order_input VALUES (2, '{\"n\":2}'), (1, 'null'), (3, NULL), (4, '{\"n\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_agg(payload ORDER BY ordinal) FROM jsonb_aggregate_order_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-jsonb-aggregate-order-null-inputs",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        3802
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(
            "[null, {\"n\": 2}, null, {\"n\": 1}]".to_owned()
        )]]
    );
}

#[test]
fn should_skip_null_values_in_strict_jsonb_object_aggregation() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_jsonb_object_strict_input (key TEXT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_jsonb_object_strict_input VALUES ('a', NULL), ('b', 'null'), ('c', '1')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_object_agg_strict(key, payload) FROM issue_811_jsonb_object_strict_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-jsonb-object-aggregate-strict-null-policy",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"c\": 1}".to_owned())]]
    );
}

#[test]
fn should_skip_sql_nulls_in_the_json_strict_aggregate() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_json_aggregate_strict_values (value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_json_aggregate_strict_values VALUES (1), (NULL), (2)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT json_agg_strict(value) FROM issue_811_json_aggregate_strict_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-811-json-aggregate-strict", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (114, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned())]]
    );
}

#[test]
fn should_strip_jsonb_null_fields() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_strip_nulls_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_strip_nulls_values VALUES ('{\"a\":null,\"items\":[null,1]}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_strip_nulls(document)::TEXT FROM issue_810_jsonb_strip_nulls_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-strip-null-arrays", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"items\": [null, 1]}".to_owned())]]
    );
}

#[test]
fn should_strip_null_elements_from_jsonb_arrays_when_requested() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_strip_array_nulls_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_strip_array_nulls_values VALUES ('{\"a\":null,\"items\":[null,1]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_strip_nulls(document, TRUE)::TEXT FROM issue_810_jsonb_strip_array_nulls_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-strip-array-null-elements",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"items\": [1]}".to_owned())]]
    );
}

#[test]
fn should_type_json_table_missing_or_json_null_fields_as_sql_null() {
    // Arrange
    let sql = r#"SELECT id, required, optional, ord FROM JSON_TABLE('[{"id":7,"required":"one","optional":null},{"id":9,"required":"two"}]', '$[*]' COLUMNS (id BIGINT PATH '$.id', required TEXT PATH '$.required', optional TEXT PATH '$.optional', ord FOR ORDINALITY)) AS j ORDER BY ord"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-column-types", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("typed JSON_TABLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 25, 25, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("one".to_owned()),
                None,
                Some("1".to_owned()),
            ],
            vec![
                Some("9".to_owned()),
                Some("two".to_owned()),
                None,
                Some("2".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_order_distinct_jsonb_aggregate_inputs_with_sql_null() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_jsonb_distinct_order (ordinal INT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_jsonb_distinct_order VALUES (2, '{\"n\":2}'), (4, NULL), (3, '{\"n\":2}'), (1, '{\"n\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_agg(DISTINCT payload ORDER BY payload) FROM issue_811_jsonb_distinct_order",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-jsonb-aggregate-distinct-order",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[{\"n\": 1}, {\"n\": 2}, null]".to_owned())]]
    );
}
