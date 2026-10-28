
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
